//! hiraku-tui: interface de terminal (ratatui) sobre o `hiraku-core`.
//!
//! - `app`: estado e teclas, sem I/O (testável).
//! - `catalog`: módulos e opções (validar, avançar, formatar valores).
//! - `wizard`: assistente de instalação (primeira execução ou `w`).
//! - `ui`: desenho das telas.
//! - `services`: leitura/gravação de estado e o worker do sync.
//! - este arquivo: o laço de eventos e a suspensão do terminal para o `sudo -v`.

pub mod app;
pub mod wizard;

pub use hiraku_hub::{catalog, services};
pub mod ui;

use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use hiraku_core::sync::SyncReport;
use hiraku_core::{Elevator, Event as CoreEvent, SudoElevator};
use ratatui::crossterm::event::{self as terminal_event, KeyEventKind};
use ratatui::DefaultTerminal;

use crate::app::{App, Command, Screen};
use crate::services::{Keepalive, Services, WorkerMsg};

const TICK: Duration = Duration::from_millis(100);

struct Running {
    rx: Receiver<WorkerMsg>,
    /// Mantém o sudo "quente" enquanto o sync roda (só existe se pedimos root).
    _keepalive: Option<Keepalive>,
}

pub fn run(services: Services) -> Result<(), String> {
    let mut terminal = ratatui::init();
    let outcome = event_loop(&mut terminal, &services);
    ratatui::restore();
    outcome
}

fn reload(app: &mut App, services: &Services) {
    match services.snapshot() {
        Ok(snapshot) => app.apply_snapshot(snapshot),
        Err(e) => app.status = format!("erro ao ler o estado: {e}"),
    }
}

fn event_loop(terminal: &mut DefaultTerminal, services: &Services) -> Result<(), String> {
    let mut app = App::new();
    app.can_root = services.can_root();
    app.env_label = services.env_label();
    let first_run = services.is_first_run();
    reload(&mut app, services);
    if first_run {
        app.start_wizard(true);
    }
    let mut running: Option<Running> = None;

    while !app.should_quit {
        terminal
            .draw(|frame| ui::draw(frame, &app))
            .map_err(|e| format!("falha ao desenhar: {e}"))?;

        drain_worker(&mut app, services, &mut running);

        if let Some(path) = app.needs_diff() {
            let result = services.diff(&path);
            app.set_diff(path, result);
        }

        if let Some((module, key, value)) = app.preview_request() {
            let colors = services.preview(&module, &key, &value);
            app.set_preview(module, key, value, colors);
        }

        if let Some(module) = app.due_apply(Instant::now()) {
            apply_live(&module, &mut app, services, &mut running);
        }

        if terminal_event::poll(TICK).map_err(|e| e.to_string())? {
            if let terminal_event::Event::Key(key) = terminal_event::read().map_err(|e| e.to_string())? {
                if key.kind == KeyEventKind::Press {
                    if let Some(command) = app.on_key(key) {
                        execute(command, &mut app, services, terminal, &mut running);
                    }
                }
            }
        }
    }

    // Saída forçada (Ctrl-C) com sync em andamento: o lock é nosso, então o liberamos
    // em vez de deixar o próximo sync esperar o lock expirar.
    if running.is_some() {
        let _ = std::fs::remove_file(services.state_dir.join("sync.lock"));
    }
    Ok(())
}

fn drain_worker(app: &mut App, services: &Services, running: &mut Option<Running>) {
    let mut done: Option<Result<SyncReport, String>> = None;
    if let Some(worker) = running.as_ref() {
        loop {
            match worker.rx.try_recv() {
                Ok(WorkerMsg::Event(event)) => {
                    // O progresso de download inundaria o log.
                    if !matches!(event, CoreEvent::Progress { .. }) {
                        app.push_log(event.describe());
                    }
                }
                Ok(WorkerMsg::Done(result)) => {
                    done = Some(result);
                    break;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    done = Some(Err("o sync terminou inesperadamente".to_string()));
                    break;
                }
            }
        }
    }
    if let Some(result) = done {
        *running = None; // solta o keepalive
        app.finish_sync(result);
        reload(app, services);
    }
}

fn execute(
    command: Command,
    app: &mut App,
    services: &Services,
    terminal: &mut DefaultTerminal,
    running: &mut Option<Running>,
) {
    match command {
        Command::Reload => {
            reload(app, services);
            app.status = "estado recarregado".to_string();
        }
        Command::ToggleEnable(id) => {
            app.status = match services.toggle_enable(&id) {
                Ok(true) => format!("{id}: habilitado (vale no próximo sync)"),
                Ok(false) => format!("{id}: desabilitado (o que já foi instalado permanece)"),
                Err(e) => format!("erro: {e}"),
            };
            reload(app, services);
        }
        Command::Resolve { path, resolution } => {
            app.status = match services.resolve(&path, resolution) {
                Ok(()) => format!("conflito resolvido: {path}"),
                Err(e) => format!("não foi possível resolver: {e}"),
            };
            reload(app, services);
        }
        Command::StartSync => start_sync(app, services, terminal, running),
        Command::SetOption { module, key, value } => {
            match services.set_option(&module, &key, value.as_deref()) {
                Ok(()) => {
                    app.status = match &value {
                        Some(v) => format!("{module}.{key} = {v}"),
                        None => format!("{module}.{key} voltou ao padrão"),
                    };
                    reload(app, services);
                    app.queue_apply(&module, Instant::now());
                }
                Err(e) => app.status = format!("não gravei: {e}"),
            }
        }
        Command::SetPin { module, pin } => {
            app.status = match services.set_pin(&module, pin.as_deref()) {
                Ok(()) => match &pin {
                    Some(p) => format!("{module}: versão fixada em {p} (configs continuam sincronizando)"),
                    None => format!("{module}: pin removido, volta a acompanhar o registry"),
                },
                Err(e) => format!("erro: {e}"),
            };
            reload(app, services);
        }
        Command::FinishWizard { settings, every } => {
            if let Err(e) = services.save_settings(&settings) {
                app.status = format!("não consegui gravar as configurações: {e}");
                return;
            }
            let schedule_note = match services.set_schedule(every) {
                Ok(note) => note,
                Err(e) => format!("agendamento não configurado: {e}"),
            };
            reload(app, services);
            app.screen = Screen::Report;
            start_sync(app, services, terminal, running);
            app.push_log(format!("configurações gravadas; {schedule_note}"));
        }
    }
}

/// Aplica a mudança de opção na hora: sync só do módulo (e dependências). Nunca
/// pede senha: se precisar de root, deixa para o `s`.
fn apply_live(module: &str, app: &mut App, services: &Services, running: &mut Option<Running>) {
    if running.is_some() {
        return;
    }
    let Ok((mut settings, lock)) = services.settings_and_lock() else {
        return;
    };
    settings.modules = vec![module.to_string()];
    if !services.modules_needing_root(&settings, &lock).unwrap_or_default().is_empty() {
        app.status = format!("{module} precisa de root para aplicar: pressione s");
        return;
    }
    app.log.clear();
    app.running = true;
    app.status = format!("aplicando {module}…");
    *running = Some(Running {
        rx: services.spawn_sync(settings),
        _keepalive: None,
    });
}

fn start_sync(
    app: &mut App,
    services: &Services,
    terminal: &mut DefaultTerminal,
    running: &mut Option<Running>,
) {
    if running.is_some() {
        app.status = "um sync já está em andamento".to_string();
        return;
    }
    let (settings, lock) = match services.settings_and_lock() {
        Ok(pair) => pair,
        Err(e) => {
            app.status = format!("erro: {e}");
            return;
        }
    };
    if settings.modules.is_empty() {
        app.status = "nenhum módulo habilitado: marque algum com e na tela Módulos".to_string();
        return;
    }

    // Se o sync vai precisar de root, pede AGORA, com o terminal normal: a TUI sai do
    // modo raw, o sudo pergunta a senha (que o app nunca vê) e a TUI volta.
    let mut keepalive = None;
    let needing = services.modules_needing_root(&settings, &lock).unwrap_or_default();
    if !needing.is_empty() {
        ratatui::restore();
        println!("\nO hiraku precisa de root para instalar: {}", needing.join(", "));
        let auth = SudoElevator.ensure_root();
        *terminal = ratatui::init();
        if let Err(e) = auth {
            app.status = format!("sem root, sync cancelado: {e}");
            return;
        }
        keepalive = Some(Keepalive::start());
    }

    app.log.clear();
    app.running = true;
    app.status = "sync em andamento…".to_string();
    *running = Some(Running {
        rx: services.spawn_sync(settings),
        _keepalive: keepalive,
    });
}
