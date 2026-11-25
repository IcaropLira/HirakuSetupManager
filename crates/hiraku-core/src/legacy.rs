//! Sobras do `install.sh` antigo (antes do app) em `~/.local/bin`.
//!
//! O instalador antigo baixava o Neovim para `~/.local/nvim-icaro` e criava
//! `~/.local/bin/nvim` apontando para ele, e trocava `~/.local/bin/vim` por um
//! wrapper (`ICARO_VIM_WRAPPER`) que chama o vim escolhido na época. Como só alguns
//! terminais põem `~/.local/bin` na frente do PATH (o kitty abre o shell em modo de
//! login), um terminal acabava usando o nvim/vim antigo e outro o novo.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyKind {
    /// `~/.local/bin/nvim` -> `~/.local/nvim-icaro` (Neovim baixado pelo instalador antigo).
    OldNvim,
    /// Wrapper antigo do vim cujo vim de verdade não existe mais (ou é o vim-minimal).
    BrokenVimWrapper,
    /// Wrapper antigo do vim que ainda funciona (só aponta o vim real).
    VimWrapper,
    /// Link que não aponta para nada.
    Dangling,
}

#[derive(Debug, Clone, Serialize)]
pub struct Leftover {
    pub path: PathBuf,
    pub kind: LegacyKind,
    /// Para onde aponta / o vim que o wrapper chama.
    pub target: Option<PathBuf>,
}

/// O vim que o wrapper antigo chama (`REAL_VIM="..."`), se for o wrapper.
pub fn wrapper_target(path: &Path) -> Option<PathBuf> {
    let text = fs::read_to_string(path).ok()?;
    if !text.contains("ICARO_VIM_WRAPPER") {
        return None;
    }
    text.lines().find_map(|l| {
        let v = l.trim().strip_prefix("REAL_VIM=")?.trim_matches(|c| c == '"' || c == '\'');
        (!v.is_empty()).then(|| PathBuf::from(v))
    })
}

/// O vim-minimal (sem +eval) ignora toda a configuração.
fn vim_has_eval(vim: &Path) -> bool {
    std::process::Command::new(vim)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("+eval"))
        .unwrap_or(false)
}

pub fn scan(home: &Path) -> Vec<Leftover> {
    let bin = home.join(".local").join("bin");
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(&bin) else { return out };
    for e in entries.flatten() {
        let path = e.path();
        let Ok(meta) = fs::symlink_metadata(&path) else { continue };
        if meta.file_type().is_symlink() {
            let target = fs::read_link(&path).ok();
            let abs = target.as_ref().map(|t| if t.is_absolute() { t.clone() } else { bin.join(t) });
            if abs.as_ref().is_some_and(|t| t.to_string_lossy().contains("/.local/nvim-icaro/")) {
                out.push(Leftover { path, kind: LegacyKind::OldNvim, target: abs });
            } else if fs::metadata(&path).is_err() {
                out.push(Leftover { path, kind: LegacyKind::Dangling, target: abs });
            }
        } else if path.file_name().is_some_and(|n| n == "vim") {
            if let Some(real) = wrapper_target(&path) {
                let kind = if real.is_file() && vim_has_eval(&real) { LegacyKind::VimWrapper } else { LegacyKind::BrokenVimWrapper };
                out.push(Leftover { path, kind, target: Some(real) });
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// Remove o que não serve mais. O nvim antigo só sai quando o hiraku vai instalar o
/// dele (`nvim_managed`); o wrapper que ainda funciona fica.
pub fn cleanup(home: &Path, nvim_managed: bool) -> Vec<String> {
    let mut done = Vec::new();
    for l in scan(home) {
        let remove = match l.kind {
            LegacyKind::OldNvim => nvim_managed,
            LegacyKind::BrokenVimWrapper | LegacyKind::Dangling => true,
            LegacyKind::VimWrapper => false,
        };
        if remove && fs::remove_file(&l.path).is_ok() {
            let why = match l.kind {
                LegacyKind::OldNvim => "Neovim antigo do install.sh (o hiraku instala o dele)",
                LegacyKind::BrokenVimWrapper => "wrapper antigo do vim apontando para um vim que não existe ou é o vim-minimal",
                _ => "link quebrado",
            };
            done.push(format!("removido {} ({why})", l.path.display()));
        }
    }
    done
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn finds_and_cleans_the_old_installer_leftovers() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let bin = home.join(".local/bin");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(home.join(".local/nvim-icaro/bin")).unwrap();
        fs::write(home.join(".local/nvim-icaro/bin/nvim"), "").unwrap();
        symlink(home.join(".local/nvim-icaro/bin/nvim"), bin.join("nvim")).unwrap();
        symlink(home.join(".local/node-icaro/bin/node"), bin.join("node")).unwrap(); // quebrado
        fs::write(bin.join("vim"), "#!/bin/sh\n# ICARO_VIM_WRAPPER\nREAL_VIM=\"/nao/existe/vim\"\n").unwrap();
        fs::write(bin.join("outro"), "x").unwrap();

        let kinds: Vec<_> = scan(home).into_iter().map(|l| (l.path.file_name().unwrap().to_string_lossy().to_string(), l.kind)).collect();
        assert_eq!(
            kinds,
            vec![
                ("node".to_string(), LegacyKind::Dangling),
                ("nvim".to_string(), LegacyKind::OldNvim),
                ("vim".to_string(), LegacyKind::BrokenVimWrapper),
            ]
        );
        // sem o módulo nvim, o nvim antigo fica
        assert_eq!(cleanup(home, false).len(), 2);
        assert!(fs::symlink_metadata(bin.join("nvim")).is_ok());
        assert_eq!(cleanup(home, true).len(), 1);
        assert!(scan(home).is_empty());
        assert!(bin.join("outro").exists());
    }
}
