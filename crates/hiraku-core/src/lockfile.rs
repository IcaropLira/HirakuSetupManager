//! Lockfile de estado: o que o app instalou, em que versão e onde.
//! Fica em `<state_dir>/lock.toml` e é gravado de forma atômica.

use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{io_err, Error, Result};

fn default_schema() -> u32 {
    1
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lockfile {
    #[serde(default = "default_schema")]
    pub schema: u32,
    #[serde(default)]
    pub modules: BTreeMap<String, LockedModule>,
}

impl Default for Lockfile {
    fn default() -> Self {
        Lockfile {
            schema: 1,
            modules: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LockedModule {
    /// "release" ou "pkg:<gerenciador>".
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// Segundos desde a época Unix.
    pub installed_at: u64,
    /// Arquivos e diretórios criados pelo app (para status e remoção futura).
    #[serde(default)]
    pub paths: Vec<String>,
    /// Padrão de versão que bloqueia o auto-update deste módulo (ex: "0.10.*").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pin: Option<String>,
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Lockfile {
    /// Arquivo inexistente = lockfile vazio.
    pub fn load(path: &Path) -> Result<Lockfile> {
        match fs::read_to_string(path) {
            Ok(src) => toml::from_str(&src).map_err(|source| Error::Parse {
                origin: path.display().to_string(),
                source,
            }),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(Lockfile::default()),
            Err(source) => Err(Error::Io {
                path: path.to_path_buf(),
                source,
            }),
        }
    }

    /// Grava em arquivo temporário e renomeia, para nunca deixar um lock pela metade.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(io_err(parent))?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| Error::Serialize(e.to_string()))?;
        let tmp = path.with_extension("toml.tmp");
        fs::write(&tmp, text).map_err(io_err(&tmp))?;
        fs::rename(&tmp, path).map_err(io_err(path))?;
        Ok(())
    }

    pub fn record(&mut self, id: &str, module: LockedModule) {
        self.modules.insert(id.to_string(), module);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> LockedModule {
        LockedModule {
            method: "release".to_string(),
            version: Some("1.2.3".to_string()),
            sha256: Some("ab".repeat(32)),
            installed_at: 1_700_000_000,
            paths: vec!["/home/ana/.local/kitty.app".to_string()],
            pin: None,
        }
    }

    #[test]
    fn missing_file_is_empty_lock() {
        let dir = tempfile::tempdir().unwrap();
        let lock = Lockfile::load(&dir.path().join("lock.toml")).unwrap();
        assert_eq!(lock, Lockfile::default());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/lock.toml");
        let mut lock = Lockfile::default();
        lock.record("kitty", sample());
        lock.record(
            "tmux",
            LockedModule {
                method: "pkg:dnf".to_string(),
                version: None,
                sha256: None,
                installed_at: 1,
                paths: vec![],
                pin: Some("3.*".to_string()),
            },
        );
        lock.save(&path).unwrap();
        assert!(!path.with_extension("toml.tmp").exists());
        assert_eq!(Lockfile::load(&path).unwrap(), lock);
    }

    #[test]
    fn corrupt_lock_is_a_parse_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lock.toml");
        fs::write(&path, "isto não é toml = = =").unwrap();
        assert!(matches!(Lockfile::load(&path), Err(Error::Parse { .. })));
    }
}
