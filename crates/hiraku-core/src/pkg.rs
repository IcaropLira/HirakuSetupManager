//! Montagem (pura) da linha de comando de cada gerenciador de pacotes.

use crate::env::PkgManager;

/// `use_sudo`: prefixa com `sudo` quando o gerenciador exige root e ainda não somos root.
pub fn pkg_command(pm: PkgManager, package: &str, cask: bool, use_sudo: bool) -> Vec<String> {
    let base: Vec<&str> = match pm {
        PkgManager::Dnf => vec!["dnf", "install", "-y"],
        PkgManager::Apt => vec!["apt-get", "install", "-y"],
        PkgManager::Pacman => vec!["pacman", "-S", "--needed", "--noconfirm"],
        PkgManager::Zypper => vec!["zypper", "--non-interactive", "install"],
        PkgManager::Brew if cask => vec!["brew", "install", "--cask"],
        PkgManager::Brew => vec!["brew", "install"],
        PkgManager::Winget => vec![
            "winget",
            "install",
            "--exact",
            "--silent",
            "--accept-package-agreements",
            "--accept-source-agreements",
            "--id",
        ],
        PkgManager::Scoop => vec!["scoop", "install"],
    };

    let mut argv = Vec::with_capacity(base.len() + 2);
    if use_sudo && pm.needs_root() {
        argv.push("sudo".to_string());
    }
    argv.extend(base.iter().map(|s| s.to_string()));
    argv.push(package.to_string());
    argv
}

/// Comando de atualização de um pacote já instalado. `None` para os gerenciadores
/// do sistema Linux: aqueles pacotes são atualizados pelo próprio SO (evita
/// atualização parcial e pedido de root em segundo plano).
pub fn pkg_upgrade_command(pm: PkgManager, package: &str, cask: bool) -> Option<Vec<String>> {
    let base: Vec<&str> = match pm {
        PkgManager::Brew if cask => vec!["brew", "upgrade", "--cask"],
        PkgManager::Brew => vec!["brew", "upgrade"],
        PkgManager::Scoop => vec!["scoop", "update"],
        PkgManager::Winget => vec![
            "winget",
            "upgrade",
            "--exact",
            "--silent",
            "--accept-package-agreements",
            "--accept-source-agreements",
            "--id",
        ],
        PkgManager::Dnf | PkgManager::Apt | PkgManager::Pacman | PkgManager::Zypper => return None,
    };
    let mut argv: Vec<String> = base.iter().map(|s| s.to_string()).collect();
    argv.push(package.to_string());
    Some(argv)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dnf_with_and_without_sudo() {
        assert_eq!(
            pkg_command(PkgManager::Dnf, "kitty", false, true),
            ["sudo", "dnf", "install", "-y", "kitty"]
        );
        assert_eq!(
            pkg_command(PkgManager::Dnf, "kitty", false, false),
            ["dnf", "install", "-y", "kitty"]
        );
    }

    #[test]
    fn brew_never_uses_sudo_and_supports_cask() {
        assert_eq!(
            pkg_command(PkgManager::Brew, "kitty", true, true),
            ["brew", "install", "--cask", "kitty"]
        );
        assert_eq!(
            pkg_command(PkgManager::Brew, "tmux", false, true),
            ["brew", "install", "tmux"]
        );
    }

    #[test]
    fn upgrades_only_for_user_space_managers() {
        assert_eq!(
            pkg_upgrade_command(PkgManager::Brew, "kitty", true).unwrap(),
            ["brew", "upgrade", "--cask", "kitty"]
        );
        assert!(pkg_upgrade_command(PkgManager::Dnf, "kitty", false).is_none());
        assert!(pkg_upgrade_command(PkgManager::Apt, "kitty", false).is_none());
        assert!(pkg_upgrade_command(PkgManager::Pacman, "kitty", false).is_none());
        assert!(pkg_upgrade_command(PkgManager::Zypper, "kitty", false).is_none());
    }

    #[test]
    fn pacman_is_idempotent_and_non_interactive() {
        let argv = pkg_command(PkgManager::Pacman, "tmux", false, true);
        assert_eq!(argv[0], "sudo");
        assert!(argv.contains(&"--needed".to_string()));
        assert!(argv.contains(&"--noconfirm".to_string()));
    }
}
