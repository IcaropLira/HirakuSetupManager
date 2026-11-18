//! Hiraku Setup Manager: o app desktop.
//!
//! Na primeira execução mostra o assistente de instalação; depois vira o hub
//! (módulos, personalização ao vivo, conflitos, atualizações). Toda a lógica vem
//! do `hiraku-hub`/`hiraku-core`; aqui só há a ponte com a interface web (`ui/`).
//!
//! Com argumentos, o binário se comporta como o CLI `hiraku` — é assim que o sync
//! agendado (`<este binário> sync --unattended`) roda sem abrir janela.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::Mutex;
use std::time::Duration;

use hiraku_core::configs::Resolution;
use hiraku_core::env::Privilege;
use hiraku_core::settings::{Mode, Settings};
use hiraku_core::sync::SyncReport;
use hiraku_core::Event as CoreEvent;
use hiraku_hub::catalog::with_dependencies;
use hiraku_hub::model::Snapshot;
use hiraku_hub::services::{Keepalive, Services, WorkerMsg};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

struct AppState {
    services: Mutex<Services>,
    running: Mutex<bool>,
    keepalive: Mutex<Option<Keepalive>>,
}

type Res<T> = Result<T, String>;

fn services(state: &State<AppState>) -> Res<Services> {
    state
        .services
        .lock()
        .map(|s| s.clone())
        .map_err(|_| "estado interno indisponível".to_string())
}

#[derive(Serialize)]
struct Info {
    version: String,
    env_label: String,
    os: String,
    can_root: bool,
    is_root: bool,
    /// "Lembrar o sudo" ligado: instalar não pede senha.
    sudo_saved: bool,
    /// Esta máquina usa um gerenciador de pacotes que exige root (dnf/apt/...).
    pkg_needs_root: bool,
    first_run: bool,
    registry_version: u64,
    registry_source: String,
    settings_file: String,
    state_dir: String,
    running: bool,
}

#[tauri::command]
fn info(app: AppHandle, state: State<AppState>) -> Res<Info> {
    let s = services(&state)?;
    Ok(Info {
        version: app.package_info().version.to_string(),
        env_label: s.env_label(),
        os: s.env.os.as_str().to_string(),
        can_root: s.can_root(),
        is_root: s.env.privilege == Privilege::Root,
        sudo_saved: s.sudo_saved(),
        pkg_needs_root: s.env.pkg_manager.is_some_and(|p| p.needs_root()),
        first_run: s.is_first_run(),
        registry_version: s.registry_version,
        registry_source: s.registry_source.clone(),
        settings_file: s.settings_file.display().to_string(),
        state_dir: s.state_dir.display().to_string(),
        running: *state.running.lock().map_err(|e| e.to_string())?,
    })
}

#[tauri::command]
fn snapshot(state: State<AppState>) -> Res<Snapshot> {
    services(&state)?.snapshot()
}

#[tauri::command]
fn toggle_module(state: State<AppState>, id: String) -> Res<bool> {
    services(&state)?.toggle_enable(&id)
}

#[tauri::command]
fn save_settings(state: State<AppState>, settings: Settings) -> Res<()> {
    services(&state)?.save_settings(&settings)
}

#[tauri::command]
fn set_option(
    state: State<AppState>,
    module: String,
    key: String,
    value: Option<String>,
) -> Res<()> {
    services(&state)?.set_option(&module, &key, value.as_deref())
}

#[tauri::command]
fn set_pin(state: State<AppState>, module: String, pin: Option<String>) -> Res<()> {
    let pin = pin.map(|p| p.trim().to_string()).filter(|p| !p.is_empty());
    services(&state)?.set_pin(&module, pin.as_deref())
}

#[tauri::command]
fn set_mode(state: State<AppState>, mode: Option<String>) -> Res<()> {
    let s = services(&state)?;
    let (mut settings, _) = s.settings_and_lock()?;
    settings.mode = match mode.as_deref() {
        Some("user") => Some(Mode::User),
        Some("system") => Some(Mode::System),
        _ => None,
    };
    s.save_settings(&settings)
}

#[tauri::command]
fn previews(state: State<AppState>, module: String, key: String) -> Res<serde_json::Value> {
    let map = services(&state)?.all_previews(&module, &key);
    // {"nord": {"background": "#2e3440", ...}, ...}
    let json = map
        .into_iter()
        .map(|(value, colors)| {
            let colors: serde_json::Map<String, serde_json::Value> = colors
                .into_iter()
                .map(|(name, (r, g, b))| (name, format!("#{r:02x}{g:02x}{b:02x}").into()))
                .collect();
            (value, serde_json::Value::Object(colors))
        })
        .collect::<serde_json::Map<_, _>>();
    Ok(serde_json::Value::Object(json))
}

#[tauri::command]
fn set_schedule(state: State<AppState>, every: Option<u64>) -> Res<String> {
    services(&state)?.set_schedule(every)
}

#[tauri::command]
fn diff(state: State<AppState>, path: String) -> Res<String> {
    services(&state)?.diff(&path)
}

#[tauri::command]
fn resolve(state: State<AppState>, path: String, keep_mine: bool) -> Res<()> {
    let resolution = if keep_mine {
        Resolution::KeepMine
    } else {
        Resolution::UseNew
    };
    services(&state)?.resolve(&path, resolution)
}

/// Settings efetivos de um sync: tudo, ou só um módulo (aplicar ao vivo).
fn sync_settings(s: &Services, only: Option<&str>) -> Res<Settings> {
    let (mut settings, _) = s.settings_and_lock()?;
    if let Some(module) = only {
        settings.modules = vec![module.to_string()];
    }
    Ok(settings)
}

/// Módulos (com dependências) que o sync vai instalar com root.
#[tauri::command]
fn needs_root(state: State<AppState>, only: Option<String>) -> Res<Vec<String>> {
    let s = services(&state)?;
    let settings = sync_settings(&s, only.as_deref())?;
    let (_, lock) = s.settings_and_lock()?;
    if s.env.privilege == Privilege::Root {
        return Ok(Vec::new());
    }
    s.modules_needing_root(&settings, &lock)
}

#[tauri::command]
fn sudo_auth(state: State<AppState>, password: String) -> Res<()> {
    services(&state)?.sudo_authenticate(&password)?;
    let mut keepalive = state.keepalive.lock().map_err(|e| e.to_string())?;
    if keepalive.is_none() {
        *keepalive = Some(Keepalive::start());
    }
    Ok(())
}

#[tauri::command]
fn sudo_remember(state: State<AppState>, password: String, on: bool) -> Res<String> {
    services(&state)?.set_sudo_saved(&password, on)
}

#[derive(Clone, Serialize)]
struct SyncLine {
    line: String,
    module: String,
    kind: &'static str,
    done: Option<u64>,
    total: Option<u64>,
}

#[derive(Clone, Serialize)]
struct SyncDone {
    report: Option<SyncReport>,
    error: Option<String>,
    only: Option<String>,
    notes: Vec<String>,
}

fn line_of(event: &CoreEvent) -> SyncLine {
    let (module, kind, done, total) = match event {
        CoreEvent::Started { module, .. } => (module, "started", None, None),
        CoreEvent::Progress {
            module,
            done,
            total,
        } => (module, "progress", Some(*done), *total),
        CoreEvent::Log { module, .. } => (module, "log", None, None),
        CoreEvent::Skipped { module, .. } => (module, "skipped", None, None),
        CoreEvent::Conflict { module, .. } => (module, "conflict", None, None),
        CoreEvent::Finished { module } => (module, "finished", None, None),
    };
    SyncLine {
        line: event.describe(),
        module: module.clone(),
        kind,
        done,
        total,
    }
}

/// Começa um sync numa thread e devolve na hora. Progresso em `sync://line`,
/// resultado em `sync://done`. `only` = aplicar só um módulo (e dependências).
#[tauri::command]
fn start_sync(app: AppHandle, state: State<AppState>, only: Option<String>, repair: Option<bool>) -> Res<()> {
    let repair = repair.unwrap_or(false);
    {
        let mut running = state.running.lock().map_err(|e| e.to_string())?;
        if *running {
            return Err("um sync já está em andamento".to_string());
        }
        *running = true;
    }

    // Sync completo: antes, busca o registry remoto (se configurado), como o CLI.
    let mut notes = Vec::new();
    if only.is_none() {
        if let Ok(mut s) = state.services.lock() {
            match s.refresh_registry() {
                Ok(Some(note)) => notes.push(note),
                Ok(None) => {}
                Err(e) => notes.push(format!("registry remoto não atualizado: {e}")),
            }
        }
    }

    let s = match services(&state) {
        Ok(s) => s,
        Err(e) => {
            *state.running.lock().map_err(|e| e.to_string())? = false;
            return Err(e);
        }
    };
    let settings = match sync_settings(&s, only.as_deref()) {
        Ok(settings) if !settings.modules.is_empty() => settings,
        Ok(_) => {
            *state.running.lock().map_err(|e| e.to_string())? = false;
            return Err("nenhum módulo habilitado".to_string());
        }
        Err(e) => {
            *state.running.lock().map_err(|e| e.to_string())? = false;
            return Err(e);
        }
    };

    let rx = if repair { s.spawn_repair(settings) } else { s.spawn_sync(settings) };
    std::thread::spawn(move || {
        let mut last_progress = std::time::Instant::now();
        let result: Result<SyncReport, String> = loop {
            match rx.recv_timeout(Duration::from_secs(600)) {
                Ok(WorkerMsg::Event(event)) => {
                    // Progresso de download: no máximo ~10 por segundo.
                    if matches!(event, CoreEvent::Progress { .. }) {
                        if last_progress.elapsed() < Duration::from_millis(100) {
                            continue;
                        }
                        last_progress = std::time::Instant::now();
                    }
                    let _ = app.emit("sync://line", line_of(&event));
                }
                Ok(WorkerMsg::Done(result)) => break result,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => {
                    break Err("o sync terminou inesperadamente".to_string())
                }
            }
        };
        let state = app.state::<AppState>();
        if let Ok(mut running) = state.running.lock() {
            *running = false;
        }
        let (report, error) = match result {
            Ok(report) => (Some(report), None),
            Err(e) => (None, Some(e)),
        };
        let _ = app.emit(
            "sync://done",
            SyncDone {
                report,
                error,
                only,
                notes,
            },
        );
    });
    Ok(())
}

/// "Verificar integridade": o que está diferente do esperado (não grava nada).
#[tauri::command]
fn health(state: State<AppState>) -> Res<hiraku_hub::health::HealthReport> {
    services(&state)?.health()
}

#[tauri::command]
fn refresh_registry(state: State<AppState>) -> Res<Option<String>> {
    let mut s = state.services.lock().map_err(|e| e.to_string())?;
    s.refresh_registry()
}

/// Módulos que entram de fato no sync (com dependências), para o resumo do assistente.
#[tauri::command]
fn resolve_dependencies(state: State<AppState>, modules: Vec<String>) -> Res<Vec<String>> {
    let s = services(&state)?;
    let catalog = hiraku_hub::catalog::build_catalog(&s.manifests, s.env.os);
    Ok(with_dependencies(&catalog, &modules))
}

#[tauri::command]
fn open_config_dir(state: State<AppState>) -> Res<()> {
    let s = services(&state)?;
    let dir: PathBuf = s
        .settings_file
        .parent()
        .map(PathBuf::from)
        .ok_or("pasta de configuração desconhecida")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let opener = match s.env.os {
        hiraku_core::Os::Macos => "open",
        hiraku_core::Os::Windows => "explorer",
        _ => "xdg-open",
    };
    std::process::Command::new(opener)
        .arg(&dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("não consegui abrir {}: {e}", dir.display()))
}

/// Abre no navegador uma página/arquivo das releases deste app (download manual
/// quando a release não tem atualização automática). Só aceita este repositório.
#[tauri::command]
fn open_url(state: State<AppState>, url: String) -> Res<()> {
    // Só abre os downloads deste repo e as páginas de projeto declaradas no registry.
    const ALLOWED: &str = "https://github.com/IcaropLira/HirakuSetupManager/";
    let s = services(&state)?;
    let declared = s.manifests.iter().any(|(m, _)| {
        m.homepage.as_deref() == Some(url.as_str()) || m.options.values().any(|o| o.link.as_deref() == Some(url.as_str()))
    });
    if !(url.starts_with(ALLOWED) || declared) || url.contains(char::is_whitespace) {
        return Err("endereço não permitido".to_string());
    }
    let os = s.env.os;
    let mut cmd = match os {
        hiraku_core::Os::Macos => std::process::Command::new("open"),
        hiraku_core::Os::Windows => {
            let mut c = std::process::Command::new("rundll32");
            c.arg("url.dll,FileProtocolHandler");
            c
        }
        _ => std::process::Command::new("xdg-open"),
    };
    cmd.arg(&url)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("não consegui abrir o navegador: {e}"))
}

fn main() -> ExitCode {
    // Com argumentos (ex.: `sync --unattended` do agendador): vira o CLI.
    let args: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with("-psn_")) // macOS às vezes passa isto ao abrir pelo Finder
        .collect();
    if !args.is_empty() {
        return hiraku_cli::main_with(&args);
    }

    let services = match Services::open(None) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("erro: {e}");
            return ExitCode::FAILURE;
        }
    };

    let result = tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(AppState {
            services: Mutex::new(services),
            running: Mutex::new(false),
            keepalive: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            info,
            snapshot,
            toggle_module,
            save_settings,
            set_option,
            set_pin,
            set_mode,
            previews,
            set_schedule,
            diff,
            resolve,
            needs_root,
            sudo_auth,
            sudo_remember,
            health,
            start_sync,
            refresh_registry,
            resolve_dependencies,
            open_config_dir,
            open_url,
        ])
        .run(tauri::generate_context!());
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("erro ao abrir a janela: {e}");
            ExitCode::FAILURE
        }
    }
}
