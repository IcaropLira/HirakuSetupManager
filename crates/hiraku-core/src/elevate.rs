//! Obtenção de privilégio de root. Cada frontend implementa a sua estratégia.
//!
//! `SudoElevator` roda `sudo -v` com o terminal herdado: a TUI precisa ser
//! suspensa (sair do modo raw) ANTES de chamar `ensure_root`, e retomada depois.
//! O app nunca vê a senha; só reaproveita a credencial em cache do sudo.

use std::process::Command;

use crate::error::{Error, Result};

pub trait Elevator {
    fn ensure_root(&mut self) -> Result<()>;
    /// `argv` (sem "sudo") já roda sem senha? Ex.: regra do "lembrar o sudo".
    fn allows_without_password(&mut self, _argv: &[String]) -> bool {
        false
    }
}

/// Para quando já se é root, ou no modo --user.
pub struct NoElevation;

impl Elevator for NoElevation {
    fn ensure_root(&mut self) -> Result<()> {
        Ok(())
    }
}

pub struct SudoElevator;

impl Elevator for SudoElevator {
    fn ensure_root(&mut self) -> Result<()> {
        let status = Command::new("sudo")
            .arg("-v")
            .status()
            .map_err(|e| Error::Elevation(format!("não consegui executar sudo: {e}")))?;
        if status.success() {
            Ok(())
        } else {
            Err(Error::Elevation(format!("`sudo -v` terminou com {status}")))
        }
    }
    fn allows_without_password(&mut self, argv: &[String]) -> bool {
        sudo_allows(argv)
    }
}

/// Para execuções agendadas: só funciona com sudo sem senha (ou credencial em cache).
/// Nunca bloqueia esperando senha.
pub struct NonInteractiveSudo;

impl Elevator for NonInteractiveSudo {
    fn ensure_root(&mut self) -> Result<()> {
        let status = Command::new("sudo")
            .args(["-n", "true"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| Error::Elevation(format!("não consegui executar sudo: {e}")))?;
        if status.success() {
            Ok(())
        } else {
            Err(Error::Elevation(
                "precisa de root e o sudo exige senha (modo não interativo)".to_string(),
            ))
        }
    }
    fn allows_without_password(&mut self, argv: &[String]) -> bool {
        sudo_allows(argv)
    }
}

// ------------------------------------------------------------------ lembrar o sudo
//
// "Lembrar o sudo" = uma regra em /etc/sudoers.d que deixa o usuário rodar SÓ o
// `install` do gerenciador de pacotes sem senha (ex.: `/usr/bin/dnf install -y *`).
// Nenhuma senha é guardada: quem passa a não pedir é o próprio sudo.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use crate::env::PkgManager;
use crate::pkg::pkg_command;

/// Arquivo da regra. O sudo ignora nomes com '.', então o '.' do usuário vira '_'.
pub fn sudoers_file(user: &str) -> PathBuf {
    Path::new("/etc/sudoers.d").join(format!("hiraku-{}", user.replace('.', "_")))
}

/// Nome de usuário seguro para entrar numa linha do sudoers.
pub fn valid_user(user: &str) -> bool {
    let mut chars = user.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase() || c == '_')
        && user.len() <= 32
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '-' | '.'))
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .chain(["/usr/bin", "/usr/sbin", "/bin"].iter().map(PathBuf::from))
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

/// Conteúdo da regra para `user` com o gerenciador `pm` (que precisa de root).
pub fn sudo_rule(user: &str, pm: PkgManager) -> Option<String> {
    if !pm.needs_root() || !valid_user(user) {
        return None;
    }
    let mut argv = pkg_command(pm, "", false, false);
    argv.pop(); // o pacote vira o curinga
    let bin = find_in_path(&argv[0])?;
    Some(format!(
        "# Gerado pelo Hiraku Setup Manager: deixa {user} instalar pacotes do sistema sem\n\
         # digitar a senha. Só o comando abaixo; o resto do sudo continua pedindo senha.\n\
         # Para desfazer: desligue no app ou rode  sudo rm {}\n\
         {user} ALL=(root) NOPASSWD: {} {} *\n",
        sudoers_file(user).display(),
        bin.display(),
        argv[1..].join(" ")
    ))
}

/// O sudo deixa rodar `argv` (sem o "sudo" na frente) sem pedir senha agora?
/// Vale por regra NOPASSWD ou por ticket em cache.
pub fn sudo_allows(argv: &[String]) -> bool {
    sudo_check(argv, false)
}

/// Igual a `sudo_allows`, mas `rule_only` ignora o ticket em cache (`-k`): só a regra conta.
fn sudo_check(argv: &[String], rule_only: bool) -> bool {
    !argv.is_empty()
        && Command::new("sudo")
            .args(if rule_only { &["-n", "-k", "-l"][..] } else { &["-n", "-l"][..] })
            .args(argv)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
}

/// Existe regra NOPASSWD para o install de `pm`? (testa com um pacote fictício)
pub fn install_without_password(pm: PkgManager) -> bool {
    pm.needs_root() && sudo_check(&pkg_command(pm, "hiraku-probe", false, false), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_only_allows_the_install_command() {
        assert!(valid_user("icaro"));
        assert!(valid_user("joao.silva"));
        assert!(!valid_user("root ALL=(ALL)"));
        assert!(!valid_user("a\nb"));
        assert!(!valid_user(""));
        assert_eq!(sudoers_file("joao.silva"), PathBuf::from("/etc/sudoers.d/hiraku-joao_silva"));
        assert!(sudo_rule("icaro", PkgManager::Brew).is_none());
        assert!(sudo_rule("x y", PkgManager::Dnf).is_none());
        // só dá para montar a regra se o binário existir nesta máquina
        if let Some(rule) = sudo_rule("icaro", PkgManager::Apt) {
            let last = rule.lines().last().unwrap();
            assert!(last.starts_with("icaro ALL=(root) NOPASSWD: /"), "{last}");
            assert!(last.ends_with("apt-get install -y *"), "{last}");
        }
    }
}
