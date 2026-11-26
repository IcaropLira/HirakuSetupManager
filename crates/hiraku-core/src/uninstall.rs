//! Desinstalar um módulo: desfaz o que o hiraku fez nesta máquina.
//!
//! - Configurações: um arquivo que continua como o hiraku gravou volta a ser o que
//!   existia antes do hiraku (o backup da primeira vez) ou é apagado; um arquivo
//!   editado à mão vai para o backup antes de sair. Blocos (`~/.bashrc`) saem do
//!   arquivo, links da pasta do módulo são removidos. Arquivos `seed` (estado seu,
//!   ex.: `local.vim`) ficam, a menos que continuem iguais ao original.
//! - Instalações: o que foi baixado (releases, plugins via git, links em
//!   `~/.local/bin`) é apagado. Pacotes do sistema (dnf/apt/brew) ficam: o relatório
//!   diz o comando para remover, já que isso pede senha e pode afetar outros programas.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::configs::{backup_name, render_template, resolve_options};
use crate::configstate::{read_blob, ConfigState};
use crate::env::Environment;
use crate::error::{io_err, Result};
use crate::event::{Event, EventSink};
use crate::lockfile::{now_secs, Lockfile};
use crate::manifest::{is_safe_relative, ConfigMode, Manifest, Method};
use crate::paths::expand_path;

#[derive(Debug, Default, Clone, Serialize)]
pub struct UninstallReport {
    pub module: String,
    /// Apagados.
    pub removed: Vec<String>,
    /// Voltaram a ser o arquivo de antes do hiraku.
    pub restored: Vec<String>,
    /// Editados à mão: uma cópia ficou no backup antes de sair.
    pub backed_up: Vec<String>,
    /// Mantidos de propósito (seus).
    pub kept: Vec<String>,
    /// Avisos (ex.: pacote do sistema que continua instalado).
    pub notes: Vec<String>,
}

fn remove_path(p: &Path) -> std::io::Result<bool> {
    match fs::symlink_metadata(p) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
        Ok(m) if m.is_dir() => fs::remove_dir_all(p).map(|_| true),
        Ok(_) => fs::remove_file(p).map(|_| true),
    }
}

/// Apaga pastas que ficaram vazias, subindo até (sem incluir) `stop`.
fn prune_empty(mut dir: Option<&Path>, stop: &Path) {
    while let Some(d) = dir {
        if d == stop || !d.starts_with(stop) || fs::remove_dir(d).is_err() {
            break;
        }
        dir = d.parent();
    }
}

/// O backup mais antigo de `dest` deste módulo: o arquivo que existia antes do hiraku.
fn first_backup(state_dir: &Path, module: &str, dest: &Path) -> Option<PathBuf> {
    let name = backup_name(dest);
    let mut ids: Vec<(u64, PathBuf)> = fs::read_dir(state_dir.join("backups"))
        .ok()?
        .flatten()
        .filter_map(|e| {
            let id = e.file_name().to_string_lossy().parse::<u64>().ok()?;
            let p = e.path().join(module).join(&name);
            p.is_file().then_some((id, p))
        })
        .collect();
    ids.sort();
    ids.into_iter().next().map(|(_, p)| p)
}

fn remove_block(original: &str, begin: &str, end: &str) -> String {
    let mut out = Vec::new();
    let mut in_block = false;
    for line in original.lines() {
        let t = line.trim_end();
        if in_block {
            if t == end.trim_end() {
                in_block = false;
            }
            continue;
        }
        if t == begin.trim_end() {
            in_block = true;
            // tira a linha em branco que o hiraku deixou antes do bloco
            if out.last().is_some_and(|l: &&str| l.trim().is_empty()) {
                out.pop();
            }
            continue;
        }
        out.push(line);
    }
    let mut text = out.join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    text
}

#[allow(clippy::too_many_arguments)]
pub fn uninstall_module(
    manifest: &Manifest,
    module_dir: &Path,
    env: &Environment,
    choices: &BTreeMap<String, String>,
    state_dir: &Path,
    lock: &mut Lockfile,
    cstate: &mut ConfigState,
    sink: &mut dyn EventSink,
) -> Result<UninstallReport> {
    let id = manifest.id.as_str();
    let mut rep = UninstallReport { module: id.to_string(), ..Default::default() };
    let home = env.home.clone().unwrap_or_else(|| PathBuf::from("/"));
    let backup_root = state_dir.join("backups").join(format!("{}-desinstalar", now_secs())).join(id);
    let save_copy = |p: &Path| -> Result<PathBuf> {
        fs::create_dir_all(&backup_root).map_err(io_err(&backup_root))?;
        let to = backup_root.join(backup_name(p));
        fs::copy(p, &to).map_err(io_err(&to))?;
        Ok(to)
    };
    sink.emit(Event::Started { module: id.to_string(), what: "desinstalando".to_string() });

    // ---------------------------------------------------------------- configs
    let vars = resolve_options(manifest, choices).unwrap_or_default();
    let mut seed_srcs = Vec::new();
    for entry in &manifest.config {
        let Some(t) = entry.dest.resolve(env.os) else { continue };
        let Ok(dest_root) = expand_path(t, env.home.as_deref()) else { continue };
        match entry.mode {
            ConfigMode::Seed => seed_srcs.push(entry.src.clone()),
            ConfigMode::Block => {
                let Some(spec) = &entry.block else { continue };
                if let Ok(text) = fs::read_to_string(&dest_root) {
                    let cleaned = remove_block(&text, &spec.begin, &spec.end);
                    if cleaned != text {
                        save_copy(&dest_root)?;
                        fs::write(&dest_root, cleaned).map_err(io_err(&dest_root))?;
                        rep.removed.push(format!("{} (bloco do hiraku)", dest_root.display()));
                    }
                }
            }
            ConfigMode::Symlink => {
                let src_rel = if entry.src.contains("{{") { render_template(&entry.src, &vars).unwrap_or_default() } else { entry.src.clone() };
                if !is_safe_relative(&src_rel) {
                    continue;
                }
                let ours = fs::canonicalize(module_dir.join(&src_rel)).ok();
                let target = fs::read_link(&dest_root).ok();
                let points_to_registry = target.as_ref().is_some_and(|t| {
                    ours.as_ref() == Some(t) || t.to_string_lossy().contains("/hiraku/registry")
                });
                if points_to_registry && fs::remove_file(&dest_root).is_ok() {
                    rep.removed.push(dest_root.display().to_string());
                    prune_empty(dest_root.parent(), &home);
                }
            }
            _ => {}
        }
    }

    let files: Vec<(String, crate::configstate::FileState)> =
        cstate.files.iter().filter(|(_, f)| f.module == id).map(|(k, f)| (k.clone(), f.clone())).collect();
    for (key, f) in files {
        let dest = PathBuf::from(&key);
        let is_seed = seed_srcs.iter().any(|s| f.src == *s || f.src.starts_with(&format!("{s}/")));
        let current = fs::read(&dest).ok();
        let base = read_blob(state_dir, &f.base_sha256)?;
        let untouched = current.is_some() && current == base;
        match current {
            None => {}
            Some(_) if is_seed && !untouched => rep.kept.push(dest.display().to_string()),
            Some(_) if untouched => {
                if let Some(orig) = first_backup(state_dir, id, &dest) {
                    fs::copy(&orig, &dest).map_err(io_err(&dest))?;
                    rep.restored.push(dest.display().to_string());
                } else {
                    fs::remove_file(&dest).map_err(io_err(&dest))?;
                    rep.removed.push(dest.display().to_string());
                    prune_empty(dest.parent(), &home);
                }
            }
            Some(_) => {
                let to = save_copy(&dest)?;
                fs::remove_file(&dest).map_err(io_err(&dest))?;
                rep.backed_up.push(format!("{} → {}", dest.display(), to.display()));
                prune_empty(dest.parent(), &home);
            }
        }
        cstate.files.remove(&key);
    }

    // ---------------------------------------------------------------- instalações
    let keys: Vec<String> =
        lock.modules.keys().filter(|k| *k == id || k.starts_with(&format!("{id}/"))).cloned().collect();
    for key in keys {
        let Some(m) = lock.modules.remove(&key) else { continue };
        if let Some(pm) = m.method.strip_prefix("pkg:") {
            let names: Vec<String> = manifest
                .install
                .iter()
                .filter_map(|s| match &s.method {
                    Method::Pkg { names, .. } => names.get(pm).cloned(),
                    _ => None,
                })
                .collect();
            let names = if names.is_empty() { id.to_string() } else { names.join(" ") };
            let cmd = match pm {
                "dnf" => format!("sudo dnf remove {names}"),
                "apt" => format!("sudo apt remove {names}"),
                "pacman" => format!("sudo pacman -R {names}"),
                "zypper" => format!("sudo zypper remove {names}"),
                "brew" => format!("brew uninstall {names}"),
                "scoop" => format!("scoop uninstall {names}"),
                "winget" => format!("winget uninstall {names}"),
                other => format!("{other}: {names}"),
            };
            rep.notes.push(format!("{names} veio do {pm} e continua no sistema (outros programas podem usar). Para tirar: {cmd}"));
            continue;
        }
        for p in &m.paths {
            let path = PathBuf::from(p);
            // só apaga dentro da pasta pessoal (nunca algo do sistema)
            if !path.starts_with(&home) || path == home {
                continue;
            }
            match remove_path(&path) {
                Ok(true) => {
                    rep.removed.push(p.clone());
                    prune_empty(path.parent(), &home);
                }
                Ok(false) => {}
                Err(e) => rep.notes.push(format!("não consegui apagar {p}: {e}")),
            }
        }
    }
    rep.removed.sort();
    rep.removed.dedup();
    sink.emit(Event::Log {
        module: id.to_string(),
        line: format!(
            "desinstalado: {} apagado(s), {} restaurado(s), {} com cópia no backup",
            rep.removed.len(),
            rep.restored.len(),
            rep.backed_up.len()
        ),
    });
    Ok(rep)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_is_removed_with_its_blank_line() {
        let b = "# >>> hiraku >>>";
        let e = "# <<< hiraku <<<";
        let text = format!("export A=1\n\n{b}\nx\n{e}\nfim\n");
        assert_eq!(remove_block(&text, b, e), "export A=1\nfim\n");
        assert_eq!(remove_block(&format!("{b}\nx\n{e}\n"), b, e), "");
        assert_eq!(remove_block("nada\n", b, e), "nada\n");
    }
}
