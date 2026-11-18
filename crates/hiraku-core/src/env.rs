//! Detecção de ambiente: SO, arquitetura, distro, gerenciador de pacotes e privilégio.
//!
//! As partes que dependem do sistema (`Environment::detect`) são finas; a lógica
//! de decisão (`parse_os_release`, `pkg_manager_for`) é pura e testável.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    Linux,
    Macos,
    Windows,
}

impl Os {
    pub fn current() -> Result<Os> {
        match std::env::consts::OS {
            "linux" => Ok(Os::Linux),
            "macos" => Ok(Os::Macos),
            "windows" => Ok(Os::Windows),
            other => Err(Error::UnsupportedOs(other.to_string())),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Os::Linux => "linux",
            Os::Macos => "macos",
            Os::Windows => "windows",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Arch {
    #[serde(rename = "x86_64")]
    X86_64,
    #[serde(rename = "aarch64")]
    Aarch64,
}

impl Arch {
    pub fn current() -> Option<Arch> {
        match std::env::consts::ARCH {
            "x86_64" => Some(Arch::X86_64),
            "aarch64" => Some(Arch::Aarch64),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PkgManager {
    Dnf,
    Apt,
    Pacman,
    Zypper,
    Brew,
    Winget,
    Scoop,
}

impl PkgManager {
    pub const ALL: [PkgManager; 7] = [
        PkgManager::Dnf,
        PkgManager::Apt,
        PkgManager::Pacman,
        PkgManager::Zypper,
        PkgManager::Brew,
        PkgManager::Winget,
        PkgManager::Scoop,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            PkgManager::Dnf => "dnf",
            PkgManager::Apt => "apt",
            PkgManager::Pacman => "pacman",
            PkgManager::Zypper => "zypper",
            PkgManager::Brew => "brew",
            PkgManager::Winget => "winget",
            PkgManager::Scoop => "scoop",
        }
    }

    pub fn from_name(name: &str) -> Option<PkgManager> {
        PkgManager::ALL.into_iter().find(|p| p.as_str() == name)
    }

    /// Gerenciadores do sistema Linux precisam de root; brew, scoop e winget
    /// (escopo de usuário) não.
    pub fn needs_root(self) -> bool {
        matches!(
            self,
            PkgManager::Dnf | PkgManager::Apt | PkgManager::Pacman | PkgManager::Zypper
        )
    }
}

/// Capacidade de obter root. É uma heurística: só `sudo -v` confirma de verdade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Privilege {
    /// Já rodando como root (euid 0).
    Root,
    /// `sudo` existe e o usuário está em sudo/wheel/admin.
    Sudo,
    /// Sem como elevar (ou desconhecido, caso do Windows nesta fase).
    None,
}

impl Privilege {
    pub fn can_root(self) -> bool {
        !matches!(self, Privilege::None)
    }
}

#[derive(Debug, Clone)]
pub struct Environment {
    pub os: Os,
    pub arch: Option<Arch>,
    pub distro: Option<String>,
    pub pkg_manager: Option<PkgManager>,
    pub privilege: Privilege,
    /// Modo de instalação escolhido: `true` = pacotes do sistema (`--system`),
    /// `false` = tudo em user-space (`--user`). Por padrão segue `privilege`.
    pub use_root: bool,
    pub home: Option<PathBuf>,
}

impl Environment {
    pub fn detect() -> Result<Environment> {
        let os = Os::current()?;
        let release = if os == Os::Linux { read_os_release() } else { None };
        let privilege = detect_privilege(os);

        Ok(Environment {
            os,
            arch: Arch::current(),
            distro: release
                .as_ref()
                .map(|r| r.id.clone())
                .filter(|id| !id.is_empty()),
            pkg_manager: detect_pkg_manager(os, release.as_ref()),
            privilege,
            use_root: privilege.can_root(),
            home: std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(PathBuf::from),
        })
    }

    /// Equivale a `--user`: ignora pacotes do sistema.
    pub fn user_mode(mut self) -> Environment {
        self.use_root = false;
        self
    }

    /// Equivale a `--system`: só vale se houver como obter root.
    pub fn system_mode(mut self) -> Environment {
        self.use_root = self.privilege.can_root();
        self
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct OsRelease {
    pub id: String,
    pub id_like: Vec<String>,
}

pub fn parse_os_release(src: &str) -> OsRelease {
    let mut release = OsRelease::default();
    for line in src.lines() {
        let Some((key, value)) = line.trim().split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"').trim_matches('\'');
        match key {
            "ID" => release.id = value.to_lowercase(),
            "ID_LIKE" => {
                release.id_like = value.split_whitespace().map(|s| s.to_lowercase()).collect()
            }
            _ => {}
        }
    }
    release
}

pub fn pkg_manager_for(release: &OsRelease) -> Option<PkgManager> {
    let ids = std::iter::once(release.id.as_str()).chain(release.id_like.iter().map(String::as_str));
    for id in ids {
        match id {
            "fedora" | "rhel" | "centos" | "rocky" | "almalinux" => return Some(PkgManager::Dnf),
            "debian" | "ubuntu" | "linuxmint" | "pop" => return Some(PkgManager::Apt),
            "arch" | "manjaro" | "endeavouros" => return Some(PkgManager::Pacman),
            "opensuse" | "opensuse-leap" | "opensuse-tumbleweed" | "suse" | "sles" => {
                return Some(PkgManager::Zypper)
            }
            _ => {}
        }
    }
    None
}

fn read_os_release() -> Option<OsRelease> {
    ["/etc/os-release", "/usr/lib/os-release"]
        .iter()
        .find_map(|path| std::fs::read_to_string(path).ok())
        .map(|src| parse_os_release(&src))
}

pub fn command_exists(name: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|dir| {
        dir.join(name).is_file() || (cfg!(windows) && dir.join(format!("{name}.exe")).is_file())
    })
}

fn probe(candidates: &[(&str, PkgManager)]) -> Option<PkgManager> {
    candidates
        .iter()
        .find(|(bin, _)| command_exists(bin))
        .map(|(_, pm)| *pm)
}

fn detect_pkg_manager(os: Os, release: Option<&OsRelease>) -> Option<PkgManager> {
    match os {
        Os::Linux => release.and_then(pkg_manager_for).or_else(|| {
            probe(&[
                ("apt-get", PkgManager::Apt),
                ("dnf", PkgManager::Dnf),
                ("pacman", PkgManager::Pacman),
                ("zypper", PkgManager::Zypper),
            ])
        }),
        Os::Macos => {
            let has_brew = command_exists("brew")
                || Path::new("/opt/homebrew/bin/brew").is_file()
                || Path::new("/usr/local/bin/brew").is_file();
            has_brew.then_some(PkgManager::Brew)
        }
        Os::Windows => probe(&[("winget", PkgManager::Winget), ("scoop", PkgManager::Scoop)]),
    }
}

pub fn current_uid() -> Option<String> {
    run_stdout("id", &["-u"]).map(|s| s.trim().to_string())
}

fn run_stdout(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

fn detect_privilege(os: Os) -> Privilege {
    if os == Os::Windows {
        return Privilege::None;
    }
    if run_stdout("id", &["-u"]).as_deref().map(str::trim) == Some("0") {
        return Privilege::Root;
    }
    if command_exists("sudo") {
        if let Some(groups) = run_stdout("id", &["-nG"]) {
            if groups
                .split_whitespace()
                .any(|g| matches!(g, "sudo" | "wheel" | "admin"))
            {
                return Privilege::Sudo;
            }
        }
    }
    Privilege::None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fedora_os_release() {
        let r = parse_os_release("NAME=\"Fedora Linux\"\nID=fedora\nVERSION_ID=41\n");
        assert_eq!(r.id, "fedora");
        assert!(r.id_like.is_empty());
        assert_eq!(pkg_manager_for(&r), Some(PkgManager::Dnf));
    }

    #[test]
    fn derivative_uses_id_like() {
        let r = parse_os_release("ID=zorin\nID_LIKE=\"ubuntu debian\"\n");
        assert_eq!(r.id_like, vec!["ubuntu", "debian"]);
        assert_eq!(pkg_manager_for(&r), Some(PkgManager::Apt));
    }

    #[test]
    fn unknown_distro_has_no_manager() {
        let r = parse_os_release("ID=nixos\n");
        assert_eq!(pkg_manager_for(&r), None);
    }

    #[test]
    fn pkg_manager_names_roundtrip() {
        for pm in PkgManager::ALL {
            assert_eq!(PkgManager::from_name(pm.as_str()), Some(pm));
        }
        assert_eq!(PkgManager::from_name("emerge"), None);
    }

    #[test]
    fn user_and_system_mode() {
        let env = Environment {
            os: Os::Linux,
            arch: Some(Arch::X86_64),
            distro: None,
            pkg_manager: Some(PkgManager::Dnf),
            privilege: Privilege::Sudo,
            use_root: true,
            home: None,
        };
        assert!(!env.clone().user_mode().use_root);
        assert!(env.user_mode().system_mode().use_root);
    }
}
