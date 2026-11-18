//! Dados do hub prontos para exibir (TUI ou app desktop): linhas de módulos e
//! conflitos, e a foto completa do estado (`Snapshot`).

use std::path::PathBuf;

use hiraku_core::configstate::ConfigState;
use hiraku_core::env::Os;
use hiraku_core::lockfile::Lockfile;
use hiraku_core::manifest::Manifest;
use hiraku_core::settings::Settings;
use hiraku_core::sync::SyncReport;
use serde::Serialize;

use crate::catalog::ModuleInfo;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModuleRow {
    pub id: String,
    pub description: String,
    pub enabled: bool,
    /// Versão instalada, ou o método (`pkg:dnf`) quando não há versão.
    pub installed: Option<String>,
    pub pin: Option<String>,
    /// Falso se o módulo não suporta este sistema operacional.
    pub supported: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConflictRow {
    pub path: String,
    pub module: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Snapshot {
    pub modules: Vec<ModuleRow>,
    pub conflicts: Vec<ConflictRow>,
    pub report: Option<SyncReport>,
    pub catalog: Vec<ModuleInfo>,
    pub settings: Settings,
    /// Há um sync agendado no sistema (systemd/launchd/schtasks).
    pub scheduled: bool,
}

pub fn build_modules(
    manifests: &[(Manifest, PathBuf)],
    os: Os,
    settings: &Settings,
    lock: &Lockfile,
) -> Vec<ModuleRow> {
    let mut rows: Vec<ModuleRow> = manifests
        .iter()
        .map(|(m, _)| ModuleRow {
            id: m.id.clone(),
            description: m.description.clone(),
            enabled: settings.modules.iter().any(|id| id == &m.id),
            installed: lock.modules.get(&m.id).map(|l| match &l.version {
                Some(version) => version.clone(),
                None => l.method.clone(),
            }),
            pin: settings.pins.get(&m.id).cloned(),
            supported: m.supports(os),
        })
        .collect();
    rows.sort_by(|a, b| a.id.cmp(&b.id));
    rows
}

pub fn build_conflicts(state: &ConfigState) -> Vec<ConflictRow> {
    state
        .pending()
        .into_iter()
        .map(|(path, file)| ConflictRow {
            path: path.to_string(),
            module: file.module.clone(),
        })
        .collect()
}

/// Linhas legíveis do relatório do último sync.
pub fn report_lines(report: &SyncReport) -> Vec<String> {
    let mut lines = vec![format!(
        "registry v{} ({})",
        report.registry_version, report.registry_source
    )];
    for note in &report.notes {
        lines.push(format!("nota: {note}"));
    }
    for m in &report.modules {
        let major = if m.major { " [MAJOR]" } else { "" };
        lines.push(format!(
            "{} {:?}{major} {}",
            m.id,
            m.status,
            m.detail.as_deref().unwrap_or("")
        ));
        if let Some(error) = &m.error {
            lines.push(format!("    erro: {error}"));
        }
        for c in m.configs.iter().filter(|c| c.outcome != "Unchanged") {
            lines.push(format!("    {}: {}", c.outcome, c.dest));
        }
        for h in &m.hooks {
            lines.push(format!("    hook {}: {}", h.name, h.outcome));
        }
    }
    lines
}

pub fn remove_option(settings: &mut Settings, module: &str, key: &str) {
    if let Some(options) = settings.options.get_mut(module) {
        options.remove(key);
        if options.is_empty() {
            settings.options.remove(module);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hiraku_core::lockfile::LockedModule;

    #[test]
    fn module_rows_reflect_settings_and_lock() {
        let make = |id: &str, platforms: &str| {
            let toml = format!("schema = 1\nid = \"{id}\"\nname = \"{id}\"\ndescription = \"d {id}\"\nplatforms = {platforms}\n");
            (Manifest::from_toml_str(&toml, "t").unwrap(), PathBuf::new())
        };
        let manifests = vec![make("zeta", "[\"linux\"]"), make("alfa", "[\"windows\"]")];
        let mut settings = Settings::default();
        settings.enable("zeta");
        settings.pins.insert("zeta".to_string(), "1.*".to_string());
        let mut lock = Lockfile::default();
        lock.record(
            "zeta",
            LockedModule {
                method: "release".to_string(),
                version: Some("1.4.0".to_string()),
                sha256: None,
                installed_at: 1,
                paths: vec![],
                pin: None,
            },
        );
        lock.record(
            "alfa",
            LockedModule {
                method: "pkg:dnf".to_string(),
                version: None,
                sha256: None,
                installed_at: 1,
                paths: vec![],
                pin: None,
            },
        );
        let rows = build_modules(&manifests, Os::Linux, &settings, &lock);
        assert_eq!(rows[0].id, "alfa", "ordenado por id");
        assert!(!rows[0].supported);
        assert_eq!(rows[0].installed.as_deref(), Some("pkg:dnf"));
        assert!(!rows[0].enabled);
        assert_eq!(rows[1].installed.as_deref(), Some("1.4.0"));
        assert_eq!(rows[1].pin.as_deref(), Some("1.*"));
        assert!(rows[1].enabled && rows[1].supported);
    }

}
