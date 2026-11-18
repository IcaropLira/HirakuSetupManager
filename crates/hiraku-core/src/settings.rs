//! Preferências do usuário (`<config_dir>/settings.toml`): quais módulos manter
//! sincronizados, opções de cada um, pins de versão, modo e URL do registry.
//! É o arquivo que o `sync` lê; pode ser editado à mão.

use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{io_err, Error, Result};

fn default_schema() -> u32 {
    1
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Tudo em user-space, sem root.
    User,
    /// Pacotes do sistema quando houver como obter root.
    System,
}

// A ordem dos campos importa para o serializador TOML: valores simples e listas
// antes das tabelas (`pins`, `options`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_schema")]
    pub schema: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<Mode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry_url: Option<String>,
    /// Módulos mantidos em dia pelo `sync` (as dependências entram automaticamente).
    #[serde(default)]
    pub modules: Vec<String>,
    /// módulo -> padrão de versão (ex.: "0.10.*"). Bloqueia atualização do binário.
    #[serde(default)]
    pub pins: BTreeMap<String, String>,
    /// módulo -> (opção -> valor).
    #[serde(default)]
    pub options: BTreeMap<String, BTreeMap<String, String>>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            schema: 1,
            mode: None,
            registry_url: None,
            modules: Vec::new(),
            pins: BTreeMap::new(),
            options: BTreeMap::new(),
        }
    }
}

pub fn settings_path(config_dir: &Path) -> PathBuf {
    config_dir.join("settings.toml")
}

impl Settings {
    pub fn load(path: &Path) -> Result<Settings> {
        match fs::read_to_string(path) {
            Ok(src) => toml::from_str(&src)
                .map(Settings::migrate)
                .map_err(|source| Error::Parse {
                    origin: path.display().to_string(),
                    source,
                }),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(Settings::default()),
            Err(source) => Err(Error::Io {
                path: path.to_path_buf(),
                source,
            }),
        }
    }

    /// Valores que mudaram de nome entre versões do registry.
    /// (módulo, opção, antigo, novo)
    const RENAMED: &'static [(&'static str, &'static str, &'static str, &'static str)] =
        &[("nvim", "colorscheme", "rose-pine", "o_do_lol")];

    fn migrate(mut self) -> Settings {
        for (module, key, old, new) in Self::RENAMED {
            if let Some(v) = self.options.get_mut(*module).and_then(|o| o.get_mut(*key)) {
                if v == old {
                    *v = new.to_string();
                }
            }
        }
        self
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

    /// Devolve `true` se o módulo foi adicionado agora.
    pub fn enable(&mut self, id: &str) -> bool {
        if self.modules.iter().any(|m| m == id) {
            return false;
        }
        self.modules.push(id.to_string());
        true
    }

    /// Devolve `true` se o módulo estava habilitado.
    pub fn disable(&mut self, id: &str) -> bool {
        let before = self.modules.len();
        self.modules.retain(|m| m != id);
        self.modules.len() != before
    }

    pub fn set_option(&mut self, module: &str, key: &str, value: &str) {
        self.options
            .entry(module.to_string())
            .or_default()
            .insert(key.to_string(), value.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renamed_values_are_migrated_on_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        fs::write(&path, "modules = [\"nvim\"]\n[options.nvim]\ncolorscheme = \"rose-pine\"\n[options.vim]\ncolorscheme = \"rose-pine\"\n").unwrap();
        let s = Settings::load(&path).unwrap();
        assert_eq!(s.options["nvim"]["colorscheme"], "o_do_lol");
        assert_eq!(s.options["vim"]["colorscheme"], "rose-pine", "só o par declarado muda");
    }

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            Settings::load(&settings_path(dir.path())).unwrap(),
            Settings::default()
        );
    }

    #[test]
    fn roundtrip_with_nested_tables() {
        let dir = tempfile::tempdir().unwrap();
        let path = settings_path(&dir.path().join("cfg"));
        let mut s = Settings::default();
        s.mode = Some(Mode::User);
        s.registry_url = Some("https://example.com/registry.tar.gz".to_string());
        s.enable("kitty");
        s.enable("tmux");
        s.pins.insert("nvim".to_string(), "0.10.*".to_string());
        s.set_option("kitty", "style", "solid");
        s.set_option("kitty", "palette", "navy");
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), s);
    }

    #[test]
    fn hand_written_file_parses() {
        let dir = tempfile::tempdir().unwrap();
        let path = settings_path(dir.path());
        fs::write(
            &path,
            "mode = \"system\"\nmodules = [\"kitty\"]\n\n[pins]\nkitty = \"0.*\"\n\n[options.kitty]\nstyle = \"solid\"\n",
        )
        .unwrap();
        let s = Settings::load(&path).unwrap();
        assert_eq!(s.mode, Some(Mode::System));
        assert_eq!(s.options["kitty"]["style"], "solid");
        assert_eq!(s.pins["kitty"], "0.*");
    }

    #[test]
    fn enable_and_disable_are_idempotent() {
        let mut s = Settings::default();
        assert!(s.enable("a"));
        assert!(!s.enable("a"));
        assert_eq!(s.modules, vec!["a"]);
        assert!(s.disable("a"));
        assert!(!s.disable("a"));
        assert!(s.modules.is_empty());
    }
}
