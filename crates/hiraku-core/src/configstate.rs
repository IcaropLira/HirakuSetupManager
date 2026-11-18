//! Estado das configs aplicadas: para cada arquivo de destino, qual foi a última
//! versão aplicada (a *base* do merge de 3 vias) e se há uma versão nova pendente.
//! Os conteúdos ficam em `<state_dir>/blobs/<sha256>` (endereçados por hash).

use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{io_err, Error, Result};
use crate::release::sha256_hex_of;

fn default_schema() -> u32 {
    1
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigState {
    #[serde(default = "default_schema")]
    pub schema: u32,
    /// Chave: caminho absoluto do arquivo de destino.
    #[serde(default)]
    pub files: BTreeMap<String, FileState>,
}

impl Default for ConfigState {
    fn default() -> Self {
        ConfigState {
            schema: 1,
            files: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileState {
    pub module: String,
    /// `src` da entrada `[[config]]` que originou o arquivo.
    pub src: String,
    /// sha256 da última versão aplicada (a base do merge).
    pub base_sha256: String,
    /// sha256 da versão nova que não pôde ser aplicada (conflito).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_sha256: Option<String>,
    pub applied_at: u64,
}

pub fn state_path(state_dir: &Path) -> PathBuf {
    state_dir.join("config-state.toml")
}

impl ConfigState {
    pub fn load(path: &Path) -> Result<ConfigState> {
        match fs::read_to_string(path) {
            Ok(src) => toml::from_str(&src).map_err(|source| Error::Parse {
                origin: path.display().to_string(),
                source,
            }),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(ConfigState::default()),
            Err(source) => Err(Error::Io {
                path: path.to_path_buf(),
                source,
            }),
        }
    }

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

    /// Arquivos com versão nova esperando resolução.
    pub fn pending(&self) -> Vec<(&str, &FileState)> {
        self.files
            .iter()
            .filter(|(_, f)| f.pending_sha256.is_some())
            .map(|(k, f)| (k.as_str(), f))
            .collect()
    }
}

fn blob_path(state_dir: &Path, sha256: &str) -> Result<PathBuf> {
    // O hash vem de um arquivo editável pelo usuário: nunca usar como caminho sem validar.
    if sha256.len() != 64 || !sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(Error::Unsupported(format!("hash inválido no estado: '{sha256}'")));
    }
    Ok(state_dir.join("blobs").join(sha256.to_ascii_lowercase()))
}

/// Guarda `bytes` (se ainda não existir) e devolve o sha256.
pub fn store_blob(state_dir: &Path, bytes: &[u8]) -> Result<String> {
    let sha = sha256_hex_of(bytes);
    let path = blob_path(state_dir, &sha)?;
    if !path.exists() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(io_err(parent))?;
        }
        fs::write(&path, bytes).map_err(io_err(&path))?;
    }
    Ok(sha)
}

pub fn read_blob(state_dir: &Path, sha256: &str) -> Result<Option<Vec<u8>>> {
    let path = blob_path(state_dir, sha256)?;
    match fs::read(&path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(source) => Err(Error::Io { path, source }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blobs_roundtrip_and_dedupe() {
        let dir = tempfile::tempdir().unwrap();
        let a = store_blob(dir.path(), b"conteudo").unwrap();
        let b = store_blob(dir.path(), b"conteudo").unwrap();
        assert_eq!(a, b);
        assert_eq!(read_blob(dir.path(), &a).unwrap().unwrap(), b"conteudo");
        assert_eq!(read_blob(dir.path(), &"0".repeat(64)).unwrap(), None);
    }

    #[test]
    fn rejects_hash_that_could_escape_the_blob_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_blob(dir.path(), "../../etc/passwd").is_err());
    }

    #[test]
    fn state_roundtrip_and_pending() {
        let dir = tempfile::tempdir().unwrap();
        let path = state_path(dir.path());
        let mut state = ConfigState::default();
        state.files.insert(
            "/home/ana/.config/kitty/kitty.conf".to_string(),
            FileState {
                module: "kitty".to_string(),
                src: "files/kitty".to_string(),
                base_sha256: "a".repeat(64),
                pending_sha256: Some("b".repeat(64)),
                applied_at: 1,
            },
        );
        state.files.insert(
            "/home/ana/.tmux.conf".to_string(),
            FileState {
                module: "tmux".to_string(),
                src: "files/tmux.conf".to_string(),
                base_sha256: "c".repeat(64),
                pending_sha256: None,
                applied_at: 2,
            },
        );
        state.save(&path).unwrap();
        let loaded = ConfigState::load(&path).unwrap();
        assert_eq!(loaded, state);
        assert_eq!(loaded.pending().len(), 1);
        assert_eq!(loaded.pending()[0].0, "/home/ana/.config/kitty/kitty.conf");
    }

    #[test]
    fn missing_state_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            ConfigState::load(&state_path(dir.path())).unwrap(),
            ConfigState::default()
        );
    }
}
