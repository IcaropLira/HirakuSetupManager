//! `sync`: leva a máquina ao estado desejado em `settings.toml`.
//!
//! Para cada módulo habilitado (dependências primeiro): instala ou atualiza o
//! binário (respeitando pins), depois aplica as configs com merge de 3 vias.
//! Um módulo que falha não interrompe os outros; seus dependentes são pulados.
//! O estado (lockfile e configs) é gravado a cada módulo, então uma interrupção
//! não perde o que já foi feito. O resultado fica em `<state_dir>/last-sync.toml`.

use std::collections::{BTreeMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::configs::{self, FirstApply};
use crate::configstate::{state_path, ConfigState};
use crate::elevate::Elevator;
use crate::env::{Environment, Privilege};
use crate::error::{io_err, Error, Result};
use crate::event::EventSink;
use crate::hooks::{self, HookContext, HookOutcome};
use crate::installer::{self, ComponentState, InstallOutcome};
use crate::lockfile::{now_secs, Lockfile};
use crate::manifest::Manifest;
use crate::settings::Settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallStatus {
    Installed,
    Updated,
    Unchanged,
    /// Pulado por pin, por precisar de root em modo não interativo, por dependência
    /// que falhou, ou por não haver passo compatível.
    Skipped,
    /// Módulo sem instalação, só configs.
    ConfigOnly,
    /// O módulo não suporta este sistema operacional.
    Unsupported,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigResult {
    pub dest: String,
    pub outcome: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HookResult {
    pub name: String,
    /// `ran`, `skipped: <motivo>` ou `failed: <erro>`.
    pub outcome: String,
}

// Valores simples antes das listas de tabelas (exigência do serializador TOML).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModuleReport {
    pub id: String,
    pub status: InstallStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// A versão instalada mudou de major (ou de minor, em 0.x).
    #[serde(default)]
    pub major: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    pub configs: Vec<ConfigResult>,
    #[serde(default)]
    pub hooks: Vec<HookResult>,
}

impl ModuleReport {
    fn new(id: &str, status: InstallStatus) -> ModuleReport {
        ModuleReport {
            id: id.to_string(),
            status,
            detail: None,
            major: false,
            error: None,
            configs: Vec::new(),
            hooks: Vec::new(),
        }
    }

    pub fn is_failure(&self) -> bool {
        self.status == InstallStatus::Failed || self.error.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncReport {
    pub started_at: u64,
    pub finished_at: u64,
    pub registry_version: u64,
    pub registry_source: String,
    #[serde(default)]
    pub notes: Vec<String>,
    #[serde(default)]
    pub modules: Vec<ModuleReport>,
}

pub fn report_path(state_dir: &Path) -> PathBuf {
    state_dir.join("last-sync.toml")
}

impl SyncReport {
    pub fn has_failures(&self) -> bool {
        self.modules.iter().any(ModuleReport::is_failure)
    }

    /// Arquivos de config com versão nova pendente de resolução.
    pub fn conflicts(&self) -> usize {
        self.modules
            .iter()
            .flat_map(|m| &m.configs)
            .filter(|c| c.outcome == "Conflict")
            .count()
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(io_err(parent))?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| Error::Serialize(e.to_string()))?;
        fs::write(path, text).map_err(io_err(path))
    }

    pub fn load(path: &Path) -> Result<Option<SyncReport>> {
        match fs::read_to_string(path) {
            Ok(src) => toml::from_str(&src).map(Some).map_err(|source| Error::Parse {
                origin: path.display().to_string(),
                source,
            }),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(source) => Err(Error::Io {
                path: path.to_path_buf(),
                source,
            }),
        }
    }
}

/// Exclusão mútua entre execuções (sync agendado x comando manual). Um lock mais
/// velho que 2 horas é considerado abandonado.
pub struct SyncLock {
    path: PathBuf,
}

const STALE_AFTER: Duration = Duration::from_secs(2 * 60 * 60);

impl SyncLock {
    pub fn acquire(state_dir: &Path) -> Result<SyncLock> {
        fs::create_dir_all(state_dir).map_err(io_err(state_dir))?;
        let path = state_dir.join("sync.lock");
        for _ in 0..2 {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    let _ = writeln!(file, "{} {}", std::process::id(), now_secs());
                    return Ok(SyncLock { path });
                }
                Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                    let age = fs::metadata(&path)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.elapsed().ok());
                    match age {
                        Some(age) if age > STALE_AFTER => {
                            let _ = fs::remove_file(&path);
                        }
                        _ => {
                            return Err(Error::Locked(format!(
                                "se tiver certeza de que não há outro, remova {}",
                                path.display()
                            )))
                        }
                    }
                }
                Err(source) => return Err(Error::Io { path, source }),
            }
        }
        Err(Error::Locked(format!("não consegui obter {}", path.display())))
    }
}

impl Drop for SyncLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// `pin`: `1.2.3` (exato) ou prefixo com `*` no fim (`0.10.*`, `3.*`, `*`). Um `v`
/// inicial é ignorado nos dois lados.
pub fn pin_matches(pin: &str, version: &str) -> bool {
    let pin = pin.trim().trim_start_matches('v');
    let version = version.trim().trim_start_matches('v');
    if pin == "*" {
        return true;
    }
    match pin.strip_suffix('*') {
        Some(prefix) => version.starts_with(prefix),
        None => version == pin,
    }
}

fn breaking_part(version: &str) -> Vec<&str> {
    let parts: Vec<&str> = version.trim_start_matches('v').split('.').collect();
    // Em 0.x, o minor é quem quebra compatibilidade.
    let keep = if parts.first() == Some(&"0") { 2 } else { 1 };
    parts.into_iter().take(keep).collect()
}

pub fn is_major_change(from: &str, to: &str) -> bool {
    breaking_part(from) != breaking_part(to)
}

fn visit(
    id: &str,
    by_id: &BTreeMap<&str, &Manifest>,
    done: &mut HashSet<String>,
    stack: &mut Vec<String>,
    out: &mut Vec<String>,
) -> Result<()> {
    if done.contains(id) {
        return Ok(());
    }
    if stack.iter().any(|s| s == id) {
        return Err(Error::Invalid {
            id: id.to_string(),
            reason: format!("dependência circular: {} -> {id}", stack.join(" -> ")),
        });
    }
    let manifest = by_id.get(id).ok_or_else(|| Error::Invalid {
        id: id.to_string(),
        reason: "módulo não existe no registry".to_string(),
    })?;
    stack.push(id.to_string());
    for dep in &manifest.requires {
        visit(dep, by_id, done, stack, out)?;
    }
    stack.pop();
    done.insert(id.to_string());
    out.push(id.to_string());
    Ok(())
}

/// Ordem de execução (dependências primeiro, com as dependências incluídas
/// automaticamente) e a lista de ids habilitados que não existem no registry.
pub fn order_modules(
    manifests: &[(Manifest, PathBuf)],
    wanted: &[String],
) -> Result<(Vec<String>, Vec<String>)> {
    let by_id: BTreeMap<&str, &Manifest> =
        manifests.iter().map(|(m, _)| (m.id.as_str(), m)).collect();
    let mut order = Vec::new();
    let mut unknown: Vec<String> = Vec::new();
    let mut done = HashSet::new();
    let mut stack = Vec::new();
    for id in wanted {
        if !by_id.contains_key(id.as_str()) {
            if !unknown.contains(id) {
                unknown.push(id.clone());
            }
            continue;
        }
        visit(id, &by_id, &mut done, &mut stack, &mut order)?;
    }
    Ok((order, unknown))
}

/// Módulos habilitados que, num sync agora, pediriam root: pacote do sistema ainda
/// não instalado por nós. Serve para a TUI/CLI pedirem `sudo -v` ANTES de começar
/// (com o terminal liberado), em vez de falhar no meio. Vazio se já somos root, se
/// o modo é `--user` ou se não há como obter root.
pub fn modules_needing_root(
    manifests: &[(Manifest, PathBuf)],
    settings: &Settings,
    env: &Environment,
    lock: &Lockfile,
) -> Result<Vec<String>> {
    if !env.use_root || !env.privilege.can_root() || env.privilege == Privilege::Root {
        return Ok(Vec::new());
    }
    let (order, _) = order_modules(manifests, &settings.modules)?;
    let mut out = Vec::new();
    for id in order {
        let Some((manifest, module_dir)) = manifests.iter().find(|(m, _)| m.id == id) else {
            continue;
        };
        if !manifest.supports(env.os) {
            continue;
        }
        for step in manifest.select_installs(env) {
            if !step.needs_root(env) {
                continue;
            }
            let key = installer::component_key(&id, step.group_name());
            let installed = lock
                .modules
                .get(&key)
                .map_or(false, |m| m.method.starts_with("pkg:"));
            if !installed {
                out.push(id.clone());
                break;
            }
        }
        if !out.contains(&id) && hooks::root_hook_pending(manifest, module_dir, env, lock) {
            out.push(id.clone());
        }
    }
    Ok(out)
}

/// Anexa uma linha ao `sync.log` (com rotação simples em 512 KiB). Nunca falha.
pub fn append_log(state_dir: &Path, line: &str) {
    const MAX_BYTES: u64 = 512 * 1024;
    let path = state_dir.join("sync.log");
    let _ = fs::create_dir_all(state_dir);
    if let Ok(meta) = fs::metadata(&path) {
        if meta.len() > MAX_BYTES {
            let _ = fs::rename(&path, state_dir.join("sync.log.old"));
        }
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "[{}] {line}", now_secs());
    }
}

pub struct SyncContext<'a> {
    pub manifests: &'a [(Manifest, PathBuf)],
    pub settings: &'a Settings,
    pub env: &'a Environment,
    pub state_dir: &'a Path,
    pub first_apply: FirstApply,
    pub registry_version: u64,
    pub registry_source: String,
    pub notes: Vec<String>,
    /// Execução agendada: nenhum hook nem passo pode esperar por uma pergunta.
    pub unattended: bool,
}

/// Traduz o resultado dos componentes para o status do módulo no relatório.
fn apply_outcome(report: &mut ModuleReport, outcome: &InstallOutcome) {
    let any = |state: ComponentState| outcome.components.iter().any(|c| c.state == state);
    let main = outcome.components.iter().find(|c| c.group == crate::manifest::MAIN_GROUP);

    report.status = if any(ComponentState::Installed) {
        InstallStatus::Installed
    } else if any(ComponentState::Updated) {
        InstallStatus::Updated
    } else if main.map_or(false, |m| m.skip_reason.is_some()) {
        InstallStatus::Skipped
    } else {
        InstallStatus::Unchanged
    };

    if let Some(main) = main {
        if let Some(reason) = &main.skip_reason {
            report.detail = Some(reason.clone());
        } else if main.state == ComponentState::External {
            report.detail = Some("já existe no PATH (não gerenciado pelo hiraku)".to_string());
        } else {
            match (&main.from, &main.to) {
                (Some(from), Some(to)) if from != to => {
                    report.major = is_major_change(from, to);
                    report.detail = Some(format!("{from} -> {to}"));
                }
                (None, Some(to)) if main.state == ComponentState::Installed => {
                    report.detail = Some(to.clone());
                }
                _ => {}
            }
        }
    }

    let warnings: Vec<String> = outcome
        .components
        .iter()
        .filter_map(|c| c.error.as_ref().map(|e| format!("{}: {e}", c.group)))
        .collect();
    if !warnings.is_empty() {
        let text = format!("avisos: {}", warnings.join("; "));
        report.detail = Some(match report.detail.take() {
            Some(detail) => format!("{detail} ({text})"),
            None => text,
        });
    }
}

fn sync_module(
    ctx: &SyncContext,
    manifest: &Manifest,
    module_dir: &Path,
    lock: &mut Lockfile,
    cstate: &mut ConfigState,
    elevator: &mut dyn Elevator,
    sink: &mut dyn EventSink,
) -> ModuleReport {
    let id = manifest.id.as_str();
    let env = ctx.env;
    let mut report = ModuleReport::new(id, InstallStatus::Unchanged);

    if !manifest.supports(env.os) {
        report.status = InstallStatus::Unsupported;
        report.detail = Some(format!("não suporta {}", env.os.as_str()));
        return report;
    }

    if manifest.install.is_empty() {
        report.status = InstallStatus::ConfigOnly;
    } else if manifest.select_installs(env).is_empty() {
        report.status = InstallStatus::Skipped;
        report.detail = Some("nenhum passo de instalação compatível com este ambiente".to_string());
    } else {
        let pin = ctx.settings.pins.get(id).map(String::as_str);
        match installer::install_module(manifest, env, ctx.state_dir, lock, elevator, sink, pin) {
            Ok(outcome) => apply_outcome(&mut report, &outcome),
            Err(Error::Elevation(message)) => {
                report.status = InstallStatus::Skipped;
                report.detail = Some(message);
            }
            Err(e) => {
                report.status = InstallStatus::Failed;
                report.error = Some(e.to_string());
            }
        }
    }

    if report.status == InstallStatus::Failed {
        return report;
    }

    // Mantém o pin visível no `status` (a fonte da verdade é o settings.toml).
    if let Some(entry) = lock.modules.get_mut(id) {
        entry.pin = ctx.settings.pins.get(id).cloned();
    }

    let choices = ctx.settings.options.get(id).cloned().unwrap_or_default();
    match configs::apply_module_configs(
        manifest,
        module_dir,
        env,
        ctx.state_dir,
        &choices,
        ctx.first_apply,
        cstate,
        sink,
    ) {
        Ok(files) => {
            report.configs = files
                .into_iter()
                .map(|f| ConfigResult {
                    dest: f.dest.display().to_string(),
                    outcome: format!("{:?}", f.outcome),
                })
                .collect();
        }
        Err(e) => report.error = Some(format!("configs: {e}")),
    }

    if report.error.is_none() && !manifest.hook.is_empty() {
        let changed = matches!(report.status, InstallStatus::Installed | InstallStatus::Updated)
            || report.configs.iter().any(|c| {
                matches!(
                    c.outcome.as_str(),
                    "Created" | "Updated" | "Merged" | "AdoptedWithBackup" | "Linked"
                )
            });
        match configs::resolve_options(manifest, &choices) {
            Ok(vars) => {
                let hook_ctx = HookContext {
                    manifest,
                    module_dir,
                    env,
                    state_dir: ctx.state_dir,
                    vars: &vars,
                    changed,
                    unattended: ctx.unattended,
                };
                for hook in hooks::run_hooks(&hook_ctx, lock, sink) {
                    let outcome = match &hook.outcome {
                        HookOutcome::Ran => "ran".to_string(),
                        HookOutcome::Skipped(reason) => format!("skipped: {reason}"),
                        HookOutcome::Failed(error) => format!("failed: {error}"),
                    };
                    if let HookOutcome::Failed(error) = &hook.outcome {
                        if !hook.optional && report.error.is_none() {
                            report.error = Some(format!("hook '{}': {error}", hook.name));
                        }
                    }
                    report.hooks.push(HookResult {
                        name: hook.name,
                        outcome,
                    });
                }
            }
            Err(e) => report.error = Some(format!("opções: {e}")),
        }
    }
    report
}

pub fn run_sync(
    ctx: SyncContext,
    elevator: &mut dyn Elevator,
    sink: &mut dyn EventSink,
) -> Result<SyncReport> {
    let _lock = SyncLock::acquire(ctx.state_dir)?;
    let started_at = now_secs();
    let lock_path = ctx.state_dir.join("lock.toml");
    let cstate_path = state_path(ctx.state_dir);
    let mut lock = Lockfile::load(&lock_path)?;
    let mut cstate = ConfigState::load(&cstate_path)?;

    let (order, unknown) = order_modules(ctx.manifests, &ctx.settings.modules)?;
    let by_id: BTreeMap<&str, &(Manifest, PathBuf)> = ctx
        .manifests
        .iter()
        .map(|entry| (entry.0.id.as_str(), entry))
        .collect();

    let mut modules = Vec::new();
    for id in &unknown {
        let mut report = ModuleReport::new(id, InstallStatus::Failed);
        report.error = Some("módulo não existe no registry".to_string());
        modules.push(report);
    }

    let mut failed: HashSet<String> = HashSet::new();
    for id in &order {
        let (manifest, module_dir) = by_id[id.as_str()];

        let broken_dependency = manifest
            .requires
            .iter()
            .find(|dep| failed.contains(dep.as_str()));
        let report = match broken_dependency {
            Some(dep) => {
                let mut report = ModuleReport::new(id, InstallStatus::Skipped);
                report.detail = Some(format!("dependência '{dep}' falhou"));
                report
            }
            None => sync_module(
                &ctx,
                manifest,
                module_dir,
                &mut lock,
                &mut cstate,
                elevator,
                sink,
            ),
        };
        if report.is_failure() || report.detail.as_deref().map_or(false, |d| d.starts_with("dependência")) {
            failed.insert(id.clone());
        }
        modules.push(report);

        lock.save(&lock_path)?;
        cstate.save(&cstate_path)?;
    }

    let report = SyncReport {
        started_at,
        finished_at: now_secs(),
        registry_version: ctx.registry_version,
        registry_source: ctx.registry_source,
        notes: ctx.notes,
        modules,
    };
    report.save(&report_path(ctx.state_dir))?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elevate::NoElevation;
    use crate::env::{Arch, Os, PkgManager, Privilege};
    use crate::event::{Event, VecSink};
    use crate::lockfile::LockedModule;

    struct DenyElevation;
    impl Elevator for DenyElevation {
        fn ensure_root(&mut self) -> Result<()> {
            Err(Error::Elevation("sudo exige senha".to_string()))
        }
    }

    struct Fx {
        _tmp: tempfile::TempDir,
        root: PathBuf,
        state_dir: PathBuf,
        home: PathBuf,
        settings: Settings,
        manifests: Vec<(Manifest, PathBuf)>,
    }

    impl Fx {
        fn new() -> Fx {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().to_path_buf();
            let home = root.join("home");
            fs::create_dir_all(&home).unwrap();
            Fx {
                state_dir: root.join("state"),
                home,
                root,
                _tmp: tmp,
                settings: Settings::default(),
                manifests: Vec::new(),
            }
        }

        fn module(&mut self, toml: &str, files: &[(&str, &str)]) {
            let manifest = Manifest::from_toml_str(toml, "test").unwrap();
            let dir = self.root.join("mods").join(&manifest.id);
            fs::create_dir_all(dir.join("files")).unwrap();
            for (name, content) in files {
                fs::write(dir.join("files").join(name), content).unwrap();
            }
            self.manifests.push((manifest, dir));
        }

        fn env(&self, pm: Option<PkgManager>, use_root: bool) -> Environment {
            Environment {
                os: Os::Linux,
                arch: Some(Arch::X86_64),
                distro: None,
                pkg_manager: pm,
                privilege: if use_root { Privilege::Sudo } else { Privilege::None },
                use_root,
                home: Some(self.home.clone()),
            }
        }

        fn run(&self, env: &Environment, elevator: &mut dyn Elevator, sink: &mut dyn EventSink) -> Result<SyncReport> {
            run_sync(
                SyncContext {
                    manifests: &self.manifests,
                    settings: &self.settings,
                    env,
                    state_dir: &self.state_dir,
                    first_apply: FirstApply::default(),
                    registry_version: 3,
                    registry_source: "embedded".to_string(),
                    notes: vec!["nota".to_string()],
                    unattended: false,
                },
                elevator,
                sink,
            )
        }
    }

    fn config_module(id: &str, requires: &str, file: &str) -> String {
        format!(
            "schema = 1\nid = \"{id}\"\nname = \"{id}\"\nrequires = [{requires}]\n[[config]]\nsrc = \"files/{file}\"\ndest = {{ unix = \"~/.config/{id}/{file}\" }}\n"
        )
    }

    fn release_module(id: &str, version: &str, sha: &str) -> String {
        format!(
            "schema = 1\nid = \"{id}\"\nname = \"{id}\"\n[[install]]\nwhen = {{ os = \"linux\" }}\nmethod = \"release\"\nversion = \"{version}\"\nurl = \"https://example.com/{id}-{{version}}.tar.gz\"\nsha256 = \"{sha}\"\ndest = \"~/.local/{id}\"\n"
        )
    }

    fn by_id<'a>(report: &'a SyncReport, id: &str) -> &'a ModuleReport {
        report.modules.iter().find(|m| m.id == id).unwrap()
    }

    #[test]
    fn pin_patterns() {
        assert!(pin_matches("0.10.*", "0.10.2"));
        assert!(pin_matches("v0.10.*", "0.10.0"));
        assert!(!pin_matches("0.10.*", "0.11.0"));
        assert!(!pin_matches("0.10.*", "0.100.1"));
        assert!(pin_matches("1.2.3", "1.2.3"));
        assert!(!pin_matches("1.2.3", "1.2.4"));
        assert!(pin_matches("*", "9.9.9"));
        assert!(pin_matches("3.*", "3.4.5"));
    }

    #[test]
    fn major_change_detection() {
        assert!(is_major_change("1.9.0", "2.0.0"));
        assert!(!is_major_change("1.2.0", "1.9.0"));
        assert!(is_major_change("0.10.2", "0.11.0"));
        assert!(!is_major_change("0.10.2", "0.10.9"));
        assert!(!is_major_change("v2.0.1", "2.3.0"));
    }

    #[test]
    fn orders_dependencies_first_and_includes_them() {
        let mut fx = Fx::new();
        fx.module(&config_module("base", "", "a"), &[("a", "1")]);
        fx.module(&config_module("mid", "\"base\"", "a"), &[("a", "1")]);
        fx.module(&config_module("top", "\"mid\"", "a"), &[("a", "1")]);
        let (order, unknown) =
            order_modules(&fx.manifests, &["top".to_string(), "nope".to_string()]).unwrap();
        assert_eq!(order, vec!["base", "mid", "top"]);
        assert_eq!(unknown, vec!["nope"]);
    }

    #[test]
    fn dependency_cycles_are_an_error() {
        let mut fx = Fx::new();
        fx.module(&config_module("a", "\"b\"", "x"), &[("x", "1")]);
        fx.module(&config_module("b", "\"a\"", "x"), &[("x", "1")]);
        assert!(matches!(
            order_modules(&fx.manifests, &["a".to_string()]),
            Err(Error::Invalid { .. })
        ));
    }

    #[test]
    fn config_only_modules_sync_and_the_report_is_saved() {
        let mut fx = Fx::new();
        fx.module(&config_module("base", "", "a.conf"), &[("a.conf", "um\n")]);
        fx.module(&config_module("app", "\"base\"", "b.conf"), &[("b.conf", "dois\n")]);
        fx.settings.enable("app");
        let env = fx.env(None, false);

        let report = fx.run(&env, &mut NoElevation, &mut VecSink::default()).unwrap();
        assert_eq!(report.modules.len(), 2);
        assert_eq!(report.modules[0].id, "base");
        assert_eq!(by_id(&report, "app").status, InstallStatus::ConfigOnly);
        assert_eq!(by_id(&report, "app").configs[0].outcome, "Created");
        assert_eq!(
            fs::read_to_string(fx.home.join(".config/app/b.conf")).unwrap(),
            "dois\n"
        );
        assert_eq!(report.notes, vec!["nota"]);
        assert!(!report.has_failures());

        let saved = SyncReport::load(&report_path(&fx.state_dir)).unwrap().unwrap();
        assert_eq!(saved, report);

        let again = fx.run(&env, &mut NoElevation, &mut VecSink::default()).unwrap();
        assert_eq!(by_id(&again, "app").configs[0].outcome, "Unchanged");
    }

    #[test]
    fn a_failing_module_does_not_stop_the_others_and_blocks_its_dependents() {
        let mut fx = Fx::new();
        fx.module(&release_module("bad", "1.0.0", &"0".repeat(64)), &[]);
        fx.module(&config_module("child", "\"bad\"", "c.conf"), &[("c.conf", "x")]);
        fx.module(&config_module("fine", "", "f.conf"), &[("f.conf", "y")]);
        fx.settings.enable("child");
        fx.settings.enable("fine");
        let env = fx.env(None, false);

        let report = fx.run(&env, &mut NoElevation, &mut VecSink::default()).unwrap();
        assert_eq!(by_id(&report, "bad").status, InstallStatus::Failed);
        assert!(by_id(&report, "bad").error.is_some());
        assert_eq!(by_id(&report, "child").status, InstallStatus::Skipped);
        assert!(by_id(&report, "child").detail.as_deref().unwrap().contains("bad"));
        assert!(!fx.home.join(".config/child/c.conf").exists());
        assert_eq!(by_id(&report, "fine").configs[0].outcome, "Created");
        assert!(report.has_failures());
    }

    #[test]
    fn pin_skips_a_release_that_does_not_match_without_touching_the_network() {
        let mut fx = Fx::new();
        fx.module(&release_module("tool", "2.0.0", &"ab".repeat(32)), &[]);
        fx.settings.enable("tool");
        fx.settings.pins.insert("tool".to_string(), "1.*".to_string());
        let env = fx.env(None, false);

        let mut sink = VecSink::default();
        let report = fx.run(&env, &mut NoElevation, &mut sink).unwrap();
        let tool = by_id(&report, "tool");
        assert_eq!(tool.status, InstallStatus::Skipped);
        assert!(tool.detail.as_deref().unwrap().contains("1.*"));
        assert!(!sink.0.iter().any(|e| matches!(e, Event::Started { .. })));
        assert!(!report.has_failures());
    }

    #[test]
    fn denied_elevation_skips_the_module_instead_of_failing() {
        let mut fx = Fx::new();
        fx.module(
            "schema = 1\nid = \"tmux\"\nname = \"tmux\"\n[[install]]\nmethod = \"pkg\"\nnames = { dnf = \"tmux\" }\n",
            &[],
        );
        fx.settings.enable("tmux");
        let env = fx.env(Some(PkgManager::Dnf), true);

        let report = fx.run(&env, &mut DenyElevation, &mut VecSink::default()).unwrap();
        let tmux = by_id(&report, "tmux");
        assert_eq!(tmux.status, InstallStatus::Skipped);
        assert!(tmux.detail.as_deref().unwrap().contains("sudo"));
        assert!(!report.has_failures());
    }

    #[test]
    fn already_installed_system_package_is_unchanged_and_never_elevates() {
        let mut fx = Fx::new();
        fx.module(
            "schema = 1\nid = \"tmux\"\nname = \"tmux\"\n[[install]]\nmethod = \"pkg\"\nnames = { dnf = \"tmux\" }\n",
            &[],
        );
        fx.settings.enable("tmux");
        let mut lock = Lockfile::default();
        lock.record(
            "tmux",
            LockedModule {
                method: "pkg:dnf".to_string(),
                version: None,
                sha256: None,
                installed_at: 1,
                paths: vec![],
                pin: None,
            },
        );
        lock.save(&fx.state_dir.join("lock.toml")).unwrap();
        let env = fx.env(Some(PkgManager::Dnf), true);

        let report = fx.run(&env, &mut DenyElevation, &mut VecSink::default()).unwrap();
        assert_eq!(by_id(&report, "tmux").status, InstallStatus::Unchanged);
    }

    #[test]
    fn unknown_enabled_modules_are_reported_as_errors() {
        let mut fx = Fx::new();
        fx.settings.enable("fantasma");
        let env = fx.env(None, false);
        let report = fx.run(&env, &mut NoElevation, &mut VecSink::default()).unwrap();
        assert!(by_id(&report, "fantasma").error.is_some());
        assert!(report.has_failures());
    }

    #[test]
    fn pin_is_mirrored_into_the_lockfile() {
        let mut fx = Fx::new();
        fx.module(
            "schema = 1\nid = \"tmux\"\nname = \"tmux\"\n[[install]]\nmethod = \"pkg\"\nnames = { dnf = \"tmux\" }\n",
            &[],
        );
        fx.settings.enable("tmux");
        fx.settings.pins.insert("tmux".to_string(), "3.*".to_string());
        let mut lock = Lockfile::default();
        lock.record(
            "tmux",
            LockedModule {
                method: "pkg:dnf".to_string(),
                version: None,
                sha256: None,
                installed_at: 1,
                paths: vec![],
                pin: None,
            },
        );
        lock.save(&fx.state_dir.join("lock.toml")).unwrap();
        fx.run(&fx.env(Some(PkgManager::Dnf), true), &mut NoElevation, &mut VecSink::default())
            .unwrap();
        let saved = Lockfile::load(&fx.state_dir.join("lock.toml")).unwrap();
        assert_eq!(saved.modules["tmux"].pin.as_deref(), Some("3.*"));
    }

    #[test]
    fn root_is_needed_only_for_uninstalled_system_packages_in_system_mode() {
        let mut fx = Fx::new();
        let pkg = "schema = 1\nid = \"tmux\"\nname = \"tmux\"\n[[install]]\nmethod = \"pkg\"\nnames = { dnf = \"tmux\" }\n";
        fx.module(pkg, &[]);
        fx.module(&config_module("conf", "", "a"), &[("a", "1")]);
        fx.settings.enable("tmux");
        fx.settings.enable("conf");
        let lock = Lockfile::default();

        let system = fx.env(Some(PkgManager::Dnf), true);
        assert_eq!(
            modules_needing_root(&fx.manifests, &fx.settings, &system, &lock).unwrap(),
            vec!["tmux"]
        );

        // modo --user, sem gerenciador, ou já root: nunca precisa pedir
        let user = fx.env(Some(PkgManager::Dnf), false);
        assert!(modules_needing_root(&fx.manifests, &fx.settings, &user, &lock).unwrap().is_empty());
        let mut root = fx.env(Some(PkgManager::Dnf), true);
        root.privilege = Privilege::Root;
        assert!(modules_needing_root(&fx.manifests, &fx.settings, &root, &lock).unwrap().is_empty());
        let none = fx.env(None, true);
        assert!(modules_needing_root(&fx.manifests, &fx.settings, &none, &lock).unwrap().is_empty());

        // já instalado por nós: o sync não pede root
        let mut installed = Lockfile::default();
        installed.record(
            "tmux",
            LockedModule {
                method: "pkg:dnf".to_string(),
                version: None,
                sha256: None,
                installed_at: 1,
                paths: vec![],
                pin: None,
            },
        );
        assert!(modules_needing_root(&fx.manifests, &fx.settings, &system, &installed).unwrap().is_empty());
    }

    #[cfg(unix)]
    fn add_hook(fx: &Fx, index: usize, name: &str, body: &str) {
        let dir = fx.manifests[index].1.join("hooks");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(name), body).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn hooks_run_after_configs_and_report_their_outcome() {
        let mut fx = Fx::new();
        let toml = format!(
            "{}[[hook]]\nname = \"mark\"\nscript = \"hooks/mark.sh\"\non = \"once\"\n",
            config_module("app", "", "a.conf")
        );
        fx.module(&toml, &[("a.conf", "x\n")]);
        add_hook(&fx, 0, "mark.sh", "echo \"$HIRAKU_CHANGED\" > \"$HIRAKU_HOME/mark.txt\"\n");
        fx.settings.enable("app");
        let env = fx.env(None, false);

        let report = fx.run(&env, &mut NoElevation, &mut VecSink::default()).unwrap();
        let app = by_id(&report, "app");
        assert_eq!(app.hooks.len(), 1);
        assert_eq!(app.hooks[0].outcome, "ran");
        assert_eq!(fs::read_to_string(fx.home.join("mark.txt")).unwrap().trim(), "1");
        assert!(app.error.is_none());

        // `once`: o segundo sync não executa nem lista o hook
        let again = fx.run(&env, &mut NoElevation, &mut VecSink::default()).unwrap();
        assert!(by_id(&again, "app").hooks.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_failing_required_hook_marks_the_module_and_blocks_dependents() {
        let mut fx = Fx::new();
        let base = format!(
            "{}[[hook]]\nname = \"boom\"\nscript = \"hooks/boom.sh\"\non = \"always\"\n",
            config_module("base", "", "a.conf")
        );
        fx.module(&base, &[("a.conf", "x\n")]);
        add_hook(&fx, 0, "boom.sh", "exit 1\n");
        fx.module(&config_module("child", "\"base\"", "c.conf"), &[("c.conf", "y\n")]);
        fx.settings.enable("child");
        let env = fx.env(None, false);

        let report = fx.run(&env, &mut NoElevation, &mut VecSink::default()).unwrap();
        let base = by_id(&report, "base");
        assert!(base.error.as_deref().unwrap().contains("boom"));
        assert!(base.hooks[0].outcome.starts_with("failed"));
        assert_eq!(by_id(&report, "child").status, InstallStatus::Skipped);
        assert!(report.has_failures());
    }

    #[cfg(unix)]
    #[test]
    fn a_failing_optional_hook_is_only_a_warning() {
        let mut fx = Fx::new();
        let toml = format!(
            "{}[[hook]]\nname = \"meh\"\nscript = \"hooks/meh.sh\"\non = \"always\"\noptional = true\n",
            config_module("app", "", "a.conf")
        );
        fx.module(&toml, &[("a.conf", "x\n")]);
        add_hook(&fx, 0, "meh.sh", "exit 1\n");
        fx.settings.enable("app");
        let report = fx
            .run(&fx.env(None, false), &mut NoElevation, &mut VecSink::default())
            .unwrap();
        assert!(by_id(&report, "app").error.is_none());
        assert!(by_id(&report, "app").hooks[0].outcome.starts_with("failed"));
        assert!(!report.has_failures());
    }

    #[test]
    fn only_one_sync_can_run_at_a_time() {
        let dir = tempfile::tempdir().unwrap();
        let first = SyncLock::acquire(dir.path()).unwrap();
        assert!(matches!(SyncLock::acquire(dir.path()), Err(Error::Locked(_))));
        drop(first);
        assert!(SyncLock::acquire(dir.path()).is_ok());
    }

    #[test]
    fn a_held_lock_blocks_run_sync_and_is_released_afterwards() {
        let fx = Fx::new();
        let env = fx.env(None, false);
        let held = SyncLock::acquire(&fx.state_dir).unwrap();
        assert!(matches!(
            fx.run(&env, &mut NoElevation, &mut VecSink::default()),
            Err(Error::Locked(_))
        ));
        drop(held);
        assert!(fx.run(&env, &mut NoElevation, &mut VecSink::default()).is_ok());
        assert!(!fx.state_dir.join("sync.lock").exists());
    }

    #[test]
    fn log_lines_are_appended_and_rotated() {
        let dir = tempfile::tempdir().unwrap();
        append_log(dir.path(), "primeira");
        append_log(dir.path(), "segunda");
        let text = fs::read_to_string(dir.path().join("sync.log")).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(text.contains("primeira"));

        fs::write(dir.path().join("sync.log"), vec![b'x'; 600 * 1024]).unwrap();
        append_log(dir.path(), "depois da rotação");
        assert!(dir.path().join("sync.log.old").exists());
        assert_eq!(
            fs::read_to_string(dir.path().join("sync.log")).unwrap().lines().count(),
            1
        );
    }
}
