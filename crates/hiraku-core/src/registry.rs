//! Carregamento de manifests a partir de um diretório (`<dir>/<módulo>/module.toml`).
//! O registry embutido no binário e o cache remoto assinado entram depois,
//! reutilizando `check_references`.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::manifest::Manifest;

pub fn load_dir(dir: &Path) -> Result<Vec<Manifest>> {
    Ok(load_dir_with_paths(dir)?.into_iter().map(|(m, _)| m).collect())
}

/// Como `load_dir`, mas devolve também o diretório de cada módulo
/// (necessário para achar os arquivos de config em `files/`).
pub fn load_dir_with_paths(dir: &Path) -> Result<Vec<(Manifest, PathBuf)>> {
    let io_err = |path: &Path| {
        let path = path.to_path_buf();
        move |source| Error::Io { path, source }
    };

    let entries = fs::read_dir(dir).map_err(io_err(dir))?;
    let mut files: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path().join("module.toml"))
        .filter(|path| path.is_file())
        .collect();
    files.sort();

    let mut loaded = Vec::with_capacity(files.len());
    for file in files {
        let src = fs::read_to_string(&file).map_err(io_err(file.as_path()))?;
        let manifest = Manifest::from_toml_str(&src, &file.display().to_string())?;
        let module_dir = file.parent().map(Path::to_path_buf).unwrap_or_default();
        loaded.push((manifest, module_dir));
    }

    let manifests: Vec<Manifest> = loaded.iter().map(|(m, _)| m.clone()).collect();
    check_references(&manifests)?;
    Ok(loaded)
}

/// ids únicos e `requires` apontando para módulos existentes.
pub fn check_references(manifests: &[Manifest]) -> Result<()> {
    let mut ids = HashSet::new();
    for m in manifests {
        if !ids.insert(m.id.as_str()) {
            return Err(Error::Invalid {
                id: m.id.clone(),
                reason: "id duplicado no registry".to_string(),
            });
        }
    }
    for m in manifests {
        for required in &m.requires {
            if !ids.contains(required.as_str()) {
                return Err(Error::Invalid {
                    id: m.id.clone(),
                    reason: format!("requires '{required}', que não existe no registry"),
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(id: &str, requires: &[&str]) -> Manifest {
        let req = requires
            .iter()
            .map(|r| format!("\"{r}\""))
            .collect::<Vec<_>>()
            .join(", ");
        let src = format!("schema = 1\nid = \"{id}\"\nname = \"{id}\"\nrequires = [{req}]\n");
        Manifest::from_toml_str(&src, id).unwrap()
    }

    #[test]
    fn detects_duplicates_and_missing_requires() {
        assert!(check_references(&[manifest("a", &[]), manifest("a", &[])]).is_err());
        assert!(check_references(&[manifest("a", &["b"])]).is_err());
        assert!(check_references(&[manifest("a", &["b"]), manifest("b", &[])]).is_ok());
    }
}
