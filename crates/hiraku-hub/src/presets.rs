//! Escolhas prontas do assistente, iguais na TUI e no app desktop.

/// Conjunto equivalente ao `./install.sh` completo de hoje.
pub const RECOMMENDED: &[&str] = &[
    "font-jetbrains-mono",
    "kitty",
    "tmux",
    "vim",
    "nvim",
    "node",
    "jdk",
    "ripgrep",
    "shell",
    "starship",
    "eza",
    "bat",
    "zoxide",
    "fzf",
    "fastfetch",
];

pub const SIX_HOURS: u64 = 6 * 60 * 60;
pub const ONE_DAY: u64 = 24 * 60 * 60;

pub const MODE_CHOICES: [(&str, &str); 3] = [
    (
        "Automático",
        "usa os pacotes da distro se houver sudo; senão, tudo em ~/.local",
    ),
    (
        "Só no meu usuário, sem sudo",
        "tudo em ~/.local — ideal para PC de laboratório",
    ),
    (
        "Sistema, com sudo",
        "dnf/apt/pacman/brew quando possível; a senha é pedida uma vez",
    ),
];

pub const EDITOR_CHOICES: [(&str, &[&str]); 4] = [
    ("Vim e Neovim", &["vim", "nvim"]),
    ("Só Neovim", &["nvim"]),
    ("Só Vim", &["vim"]),
    ("Nenhum editor", &[]),
];

pub const UPDATE_CHOICES: [(&str, Option<u64>); 3] = [
    ("A cada 6 horas", Some(SIX_HOURS)),
    ("Uma vez por dia", Some(ONE_DAY)),
    ("Só quando eu mandar (tecla s no hub)", None),
];

