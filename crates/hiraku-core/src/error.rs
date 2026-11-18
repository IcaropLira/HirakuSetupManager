use std::path::{Path, PathBuf};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("falha ao ler/escrever {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("TOML inválido em {origin}: {source}")]
    Parse {
        origin: String,
        #[source]
        source: toml::de::Error,
    },

    #[error("falha ao serializar TOML: {0}")]
    Serialize(String),

    #[error("manifest '{id}' inválido: {reason}")]
    Invalid { id: String, reason: String },

    #[error("sistema operacional não suportado: {0}")]
    UnsupportedOs(String),

    #[error("'{id}' não tem passo de instalação compatível com este ambiente")]
    NoCompatibleStep { id: String },

    #[error("não suportado: {0}")]
    Unsupported(String),

    #[error("falha no download de {url}: {reason}")]
    Download { url: String, reason: String },

    #[error("checksum não confere: esperado {expected}, obtido {actual}")]
    ChecksumMismatch { expected: String, actual: String },

    #[error("falha ao extrair arquivo: {0}")]
    Archive(String),

    #[error("comando `{cmd}` falhou: {reason}")]
    Command { cmd: String, reason: String },

    #[error("outro hiraku já está em execução: {0}")]
    Locked(String),

    #[error("assinatura inválida: {0}")]
    Signature(String),

    #[error("não foi possível obter privilégio de root: {0}")]
    Elevation(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Atalho para `.map_err(io_err(path))`.
pub(crate) fn io_err(path: &Path) -> impl FnOnce(std::io::Error) -> Error {
    let path = path.to_path_buf();
    move |source| Error::Io { path, source }
}
