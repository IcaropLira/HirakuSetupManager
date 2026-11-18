//! Caminhos: expansão de `~`, templates de URL e diretório de estado do app.

use std::path::{Path, PathBuf};

use crate::env::{Environment, Os};
use crate::error::{Error, Result};

/// Expande `~` e `~/...`. Outros caminhos passam intactos.
pub fn expand_tilde(path: &str, home: Option<&Path>) -> Result<PathBuf> {
    if path == "~" || path.starts_with("~/") {
        let home = home.ok_or_else(|| Error::Unsupported("HOME não definido".to_string()))?;
        let rest = path.trim_start_matches('~').trim_start_matches('/');
        return Ok(if rest.is_empty() {
            home.to_path_buf()
        } else {
            home.join(rest)
        });
    }
    Ok(PathBuf::from(path))
}

/// Expande `%VAR%` (estilo Windows) usando `lookup`.
pub fn expand_vars(path: &str, lookup: &dyn Fn(&str) -> Option<String>) -> Result<String> {
    let mut out = String::with_capacity(path.len());
    let mut rest = path;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('%') else {
            return Err(Error::Unsupported(format!("'%' sem par em '{path}'")));
        };
        let name = &after[..end];
        let value = lookup(name).ok_or_else(|| {
            Error::Unsupported(format!("variável de ambiente '{name}' não definida"))
        })?;
        out.push_str(&value);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// `%VAR%` e depois `~`.
pub fn expand_path(path: &str, home: Option<&Path>) -> Result<PathBuf> {
    let expanded = expand_vars(path, &|name| std::env::var(name).ok())?;
    expand_tilde(&expanded, home)
}

pub fn render_version(template: &str, version: &str) -> String {
    template.replace("{version}", version)
}

/// Último segmento da URL, sem query nem fragmento.
pub fn file_name_from_url(url: &str) -> String {
    let clean = url.split(|c| c == '?' || c == '#').next().unwrap_or(url);
    clean
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("download")
        .to_string()
}

/// Diretório de estado (lockfile, downloads, backups). Versão pura, testável.
pub fn state_dir_for(
    os: Os,
    home: Option<&Path>,
    xdg_data_home: Option<&Path>,
    local_app_data: Option<&Path>,
) -> Option<PathBuf> {
    match os {
        Os::Linux => xdg_data_home
            .map(|p| p.join("hiraku"))
            .or_else(|| home.map(|h| h.join(".local/share/hiraku"))),
        Os::Macos => home.map(|h| h.join("Library/Application Support/hiraku")),
        Os::Windows => local_app_data.map(|p| p.join("hiraku")),
    }
}

/// Diretório de configuração do usuário (`settings.toml`). Versão pura.
pub fn config_dir_for(
    os: Os,
    home: Option<&Path>,
    xdg_config_home: Option<&Path>,
    app_data: Option<&Path>,
) -> Option<PathBuf> {
    match os {
        Os::Linux => xdg_config_home
            .map(|p| p.join("hiraku"))
            .or_else(|| home.map(|h| h.join(".config/hiraku"))),
        Os::Macos => home.map(|h| h.join("Library/Application Support/hiraku")),
        Os::Windows => app_data.map(|p| p.join("hiraku")),
    }
}

pub fn config_dir(env: &Environment) -> Option<PathBuf> {
    let xdg = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute());
    let app_data = std::env::var_os("APPDATA").map(PathBuf::from);
    config_dir_for(env.os, env.home.as_deref(), xdg.as_deref(), app_data.as_deref())
}

pub fn state_dir(env: &Environment) -> Option<PathBuf> {
    let xdg = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute());
    let local_app_data = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    state_dir_for(
        env.os,
        env.home.as_deref(),
        xdg.as_deref(),
        local_app_data.as_deref(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_tilde() {
        let home = Path::new("/home/ana");
        assert_eq!(expand_tilde("~", Some(home)).unwrap(), PathBuf::from("/home/ana"));
        assert_eq!(
            expand_tilde("~/.local/bin", Some(home)).unwrap(),
            PathBuf::from("/home/ana/.local/bin")
        );
        assert_eq!(
            expand_tilde("/opt/x", Some(home)).unwrap(),
            PathBuf::from("/opt/x")
        );
        assert!(expand_tilde("~/x", None).is_err());
    }

    #[test]
    fn expands_percent_vars() {
        let lookup = |name: &str| (name == "APPDATA").then(|| "C:/Users/ana/AppData".to_string());
        assert_eq!(
            expand_vars("%APPDATA%/kitty", &lookup).unwrap(),
            "C:/Users/ana/AppData/kitty"
        );
        assert_eq!(expand_vars("sem/vars", &lookup).unwrap(), "sem/vars");
        assert!(expand_vars("%NOPE%/x", &lookup).is_err());
        assert!(expand_vars("100%", &lookup).is_err());
    }

    #[test]
    fn renders_version_and_file_name() {
        assert_eq!(render_version("a-{version}-b-{version}", "1.2"), "a-1.2-b-1.2");
        assert_eq!(
            file_name_from_url("https://x.dev/dl/kitty-1.2.txz?raw=1"),
            "kitty-1.2.txz"
        );
        assert_eq!(file_name_from_url("https://x.dev/"), "download");
    }

    #[test]
    fn config_dir_per_os() {
        let home = Path::new("/home/ana");
        assert_eq!(
            config_dir_for(Os::Linux, Some(home), None, None),
            Some(PathBuf::from("/home/ana/.config/hiraku"))
        );
        assert_eq!(
            config_dir_for(Os::Linux, Some(home), Some(Path::new("/cfg")), None),
            Some(PathBuf::from("/cfg/hiraku"))
        );
        assert_eq!(
            config_dir_for(Os::Windows, None, None, Some(Path::new("C:/A"))),
            Some(PathBuf::from("C:/A/hiraku"))
        );
    }

    #[test]
    fn state_dir_per_os() {
        let home = Path::new("/home/ana");
        assert_eq!(
            state_dir_for(Os::Linux, Some(home), None, None),
            Some(PathBuf::from("/home/ana/.local/share/hiraku"))
        );
        assert_eq!(
            state_dir_for(Os::Linux, Some(home), Some(Path::new("/data")), None),
            Some(PathBuf::from("/data/hiraku"))
        );
        assert_eq!(
            state_dir_for(Os::Macos, Some(home), None, None),
            Some(PathBuf::from("/home/ana/Library/Application Support/hiraku"))
        );
        assert_eq!(
            state_dir_for(Os::Windows, None, None, Some(Path::new("C:/L"))),
            Some(PathBuf::from("C:/L/hiraku"))
        );
        assert_eq!(state_dir_for(Os::Windows, Some(home), None, None), None);
    }
}
