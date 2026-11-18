//! Estado da TUI e tratamento de teclas, sem nenhum I/O: `on_key` devolve um
//! `Command` e quem executa (o laço de eventos) fica em `lib.rs`. Assim a navegação
//! é testável sem terminal.

use std::time::{Duration, Instant};

use hiraku_core::configs::Resolution;
use hiraku_core::settings::Settings;
use hiraku_core::sync::SyncReport;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub use hiraku_hub::model::{
    build_conflicts, build_modules, remove_option, report_lines, ConflictRow, ModuleRow, Snapshot,
};

use crate::catalog::{ModuleInfo, OptKind, OptSpec};
use crate::wizard::{sorted_specs, Step, Wizard, WizardOut};

const MAX_LOG_LINES: usize = 200;

/// Espera depois da última mudança antes de aplicar ao vivo (←/→ rápidos viram
/// um sync só).
pub const APPLY_DELAY: Duration = Duration::from_millis(700);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Modules,
    Options,
    Conflicts,
    Report,
    Help,
}

impl Screen {
    pub const ALL: [Screen; 5] = [
        Screen::Modules,
        Screen::Options,
        Screen::Conflicts,
        Screen::Report,
        Screen::Help,
    ];

    pub fn index(self) -> usize {
        Screen::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }

    fn step(self, delta: isize) -> Screen {
        let len = Screen::ALL.len() as isize;
        let next = (self.index() as isize + delta).rem_euclid(len);
        Screen::ALL[next as usize]
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Reload,
    ToggleEnable(String),
    Resolve { path: String, resolution: Resolution },
    StartSync,
    /// `value = None` volta ao padrão do módulo.
    SetOption {
        module: String,
        key: String,
        value: Option<String>,
    },
    /// `pin = None` remove o pin.
    SetPin { module: String, pin: Option<String> },
    /// Fim do assistente: grava, ajusta o agendamento e roda o sync.
    FinishWizard {
        settings: Settings,
        every: Option<u64>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum EditTarget {
    Option(OptSpec),
    Pin(String),
}

/// Janela por cima da tela: lista para escolher ou campo de texto.
#[derive(Debug, Clone, PartialEq)]
pub enum Editor {
    Pick {
        spec: OptSpec,
        values: Vec<String>,
        sel: usize,
    },
    Input {
        target: EditTarget,
        buffer: String,
        error: Option<String>,
    },
}

/// Cores extraídas do arquivo que a opção escolhe (ex.: paleta do kitty).
#[derive(Debug, Clone, PartialEq)]
pub struct Preview {
    pub module: String,
    pub key: String,
    pub value: String,
    pub colors: Vec<(String, (u8, u8, u8))>,
}

impl Preview {
    pub fn color(&self, name: &str) -> Option<(u8, u8, u8)> {
        self.colors.iter().find(|(n, _)| n == name).map(|(_, c)| *c)
    }
}

pub struct App {
    pub screen: Screen,
    pub modules: Vec<ModuleRow>,
    pub module_sel: usize,
    pub conflicts: Vec<ConflictRow>,
    pub conflict_sel: usize,
    pub diff_path: Option<String>,
    pub diff_text: String,
    pub diff_scroll: u16,
    pub report: Option<SyncReport>,
    pub report_scroll: u16,
    pub log: Vec<String>,
    pub running: bool,
    pub status: String,
    pub should_quit: bool,

    pub catalog: Vec<ModuleInfo>,
    pub settings: Settings,
    pub scheduled: bool,
    pub option_sel: usize,
    pub editor: Option<Editor>,
    pub wizard: Option<Wizard>,
    pub preview: Option<Preview>,
    /// Há como obter root (informado pelo laço; usado pelo assistente).
    pub can_root: bool,
    /// Texto curto do ambiente ("Fedora 41 · x86_64 · sudo disponível").
    pub env_label: String,
    /// Aplicar mudanças de opção na hora (sync só do módulo).
    pub live: bool,
    pub pending_apply: Option<(String, Instant)>,
}

fn move_selection(selected: &mut usize, len: usize, delta: isize) {
    if len == 0 {
        *selected = 0;
        return;
    }
    let next = (*selected as isize + delta).clamp(0, len as isize - 1);
    *selected = next as usize;
}

impl App {
    pub fn new() -> App {
        App {
            screen: Screen::Modules,
            modules: Vec::new(),
            module_sel: 0,
            conflicts: Vec::new(),
            conflict_sel: 0,
            diff_path: None,
            diff_text: String::new(),
            diff_scroll: 0,
            report: None,
            report_scroll: 0,
            log: Vec::new(),
            running: false,
            status: "pronto — ? para ajuda".to_string(),
            should_quit: false,
            catalog: Vec::new(),
            settings: Settings::default(),
            scheduled: false,
            option_sel: 0,
            editor: None,
            wizard: None,
            preview: None,
            can_root: false,
            env_label: String::new(),
            live: true,
            pending_apply: None,
        }
    }

    pub fn apply_snapshot(&mut self, snapshot: Snapshot) {
        self.modules = snapshot.modules;
        self.conflicts = snapshot.conflicts;
        self.report = snapshot.report;
        self.catalog = snapshot.catalog;
        self.settings = snapshot.settings;
        self.scheduled = snapshot.scheduled;
        self.module_sel = self.module_sel.min(self.modules.len().saturating_sub(1));
        self.conflict_sel = self.conflict_sel.min(self.conflicts.len().saturating_sub(1));
        self.option_sel = self.option_sel.min(self.option_specs().len().saturating_sub(1));
        // O diff é recarregado sob demanda (`needs_diff`).
        self.diff_path = None;
        self.diff_text.clear();
        self.diff_scroll = 0;
    }

    pub fn selected_module(&self) -> Option<&ModuleRow> {
        self.modules.get(self.module_sel)
    }

    pub fn selected_conflict(&self) -> Option<&ConflictRow> {
        self.conflicts.get(self.conflict_sel)
    }

    /// Opções de todos os módulos que rodam neste sistema (tela Opções).
    pub fn option_specs(&self) -> Vec<OptSpec> {
        sorted_specs(self.catalog.iter().filter(|m| m.supported))
    }

    pub fn is_enabled(&self, id: &str) -> bool {
        self.settings.modules.iter().any(|m| m == id)
    }

    pub fn is_installed(&self, id: &str) -> bool {
        self.modules
            .iter()
            .any(|m| m.id == id && m.installed.is_some())
    }

    /// Caminho cujo diff precisa ser (re)carregado, se for o caso.
    pub fn needs_diff(&self) -> Option<String> {
        if self.screen != Screen::Conflicts || self.wizard.is_some() {
            return None;
        }
        let path = &self.selected_conflict()?.path;
        (self.diff_path.as_deref() != Some(path.as_str())).then(|| path.clone())
    }

    pub fn set_diff(&mut self, path: String, result: Result<String, String>) {
        self.diff_text = match result {
            Ok(text) if text.trim().is_empty() => "(sem diferenças)".to_string(),
            Ok(text) => text,
            Err(e) => format!("não foi possível gerar o diff: {e}"),
        };
        self.diff_path = Some(path);
        self.diff_scroll = 0;
    }

    /// Opção de escolha em foco e o valor que deve ser pré-visualizado: o item
    /// destacado na lista aberta, ou o valor atual da linha selecionada.
    pub fn focused_choice(&self) -> Option<(OptSpec, String)> {
        if let Some(Editor::Pick { spec, values, sel }) = &self.editor {
            return values.get(*sel).map(|v| (spec.clone(), v.clone()));
        }
        let (spec, values) = if let Some(w) = &self.wizard {
            if w.step != Step::Options {
                return None;
            }
            (w.option_specs().get(w.option_sel)?.clone(), &w.draft)
        } else if self.screen == Screen::Options {
            (self.option_specs().get(self.option_sel)?.clone(), &self.settings)
        } else {
            return None;
        };
        if !matches!(spec.kind, OptKind::Choice(_)) {
            return None;
        }
        let value = spec.value_in(values).0;
        Some((spec, value))
    }

    /// (módulo, opção, valor) cuja pré-visualização precisa ser carregada.
    pub fn preview_request(&self) -> Option<(String, String, String)> {
        let (spec, value) = self.focused_choice()?;
        let same = self
            .preview
            .as_ref()
            .is_some_and(|p| p.module == spec.module && p.key == spec.key && p.value == value);
        (!same).then_some((spec.module, spec.key, value))
    }

    pub fn set_preview(&mut self, module: String, key: String, value: String, colors: Vec<(String, (u8, u8, u8))>) {
        self.preview = Some(Preview {
            module,
            key,
            value,
            colors,
        });
    }

    /// Pré-visualização válida para o que está em foco agora.
    pub fn current_preview(&self) -> Option<&Preview> {
        let (spec, value) = self.focused_choice()?;
        self.preview
            .as_ref()
            .filter(|p| p.module == spec.module && p.key == spec.key && p.value == value && !p.colors.is_empty())
    }

    pub fn push_log(&mut self, line: String) {
        self.log.push(line);
        if self.log.len() > MAX_LOG_LINES {
            let excess = self.log.len() - MAX_LOG_LINES;
            self.log.drain(..excess);
        }
    }

    pub fn finish_sync(&mut self, result: Result<SyncReport, String>) {
        self.running = false;
        self.status = match result {
            Ok(report) => {
                let failed = report.has_failures();
                let conflicts = report.conflicts();
                format!(
                    "sync concluído: {} módulo(s), {conflicts} conflito(s){}",
                    report.modules.len(),
                    if failed { ", COM FALHAS (tela Relatório)" } else { "" }
                )
            }
            Err(e) => format!("sync falhou: {e}"),
        };
    }

    pub fn start_wizard(&mut self, first_run: bool) {
        self.editor = None;
        self.wizard = Some(Wizard::new(
            &self.catalog,
            &self.settings,
            first_run,
            self.can_root,
            self.scheduled,
        ));
    }

    /// Agenda a aplicação ao vivo de uma opção recém-gravada (se fizer sentido).
    pub fn queue_apply(&mut self, module: &str, now: Instant) {
        if !self.live {
            return;
        }
        if !self.is_enabled(module) {
            self.status.push_str(" — módulo desabilitado: habilite em Módulos para aplicar");
            return;
        }
        if !self.is_installed(module) {
            self.status.push_str(" — ainda não instalado: s instala e aplica");
            return;
        }
        self.pending_apply = Some((module.to_string(), now));
    }

    /// Módulo cuja aplicação ao vivo já pode rodar.
    pub fn due_apply(&mut self, now: Instant) -> Option<String> {
        if self.running {
            return None;
        }
        match &self.pending_apply {
            Some((_, at)) if now.duration_since(*at) >= APPLY_DELAY => {
                self.pending_apply.take().map(|(m, _)| m)
            }
            _ => None,
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) -> Option<Command> {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return None;
        }
        if self.editor.is_some() {
            return self.on_editor_key(key.code, key.modifiers);
        }
        if self.wizard.is_some() {
            return self.on_wizard_key(key.code, key.modifiers);
        }
        match key.code {
            KeyCode::Char('q') => {
                if self.running {
                    self.status =
                        "sync em andamento: aguarde terminar (Ctrl-C força a saída)".to_string();
                } else {
                    self.should_quit = true;
                }
                return None;
            }
            KeyCode::Tab => {
                self.screen = self.screen.step(1);
                return None;
            }
            KeyCode::BackTab => {
                self.screen = self.screen.step(-1);
                return None;
            }
            KeyCode::Char(c @ '1'..='5') => {
                self.screen = Screen::ALL[c as usize - '1' as usize];
                return None;
            }
            KeyCode::Char('?') => {
                self.screen = Screen::Help;
                return None;
            }
            KeyCode::Char('r') => return Some(Command::Reload),
            KeyCode::Char('s') => {
                if self.running {
                    self.status = "um sync já está em andamento".to_string();
                    return None;
                }
                self.pending_apply = None;
                return Some(Command::StartSync);
            }
            KeyCode::Char('w') => {
                if self.running {
                    self.status = "aguarde o sync terminar para abrir o assistente".to_string();
                } else {
                    self.start_wizard(false);
                }
                return None;
            }
            _ => {}
        }

        match self.screen {
            Screen::Modules => self.on_modules_key(key.code),
            Screen::Options => self.on_option_key(key.code, key.modifiers),
            Screen::Conflicts => self.on_conflicts_key(key.code),
            Screen::Report => {
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        self.report_scroll = self.report_scroll.saturating_sub(1)
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        self.report_scroll = self.report_scroll.saturating_add(1)
                    }
                    _ => {}
                }
                None
            }
            Screen::Help => None,
        }
    }

    fn on_modules_key(&mut self, code: KeyCode) -> Option<Command> {
        match code {
            KeyCode::Up | KeyCode::Char('k') => move_selection(&mut self.module_sel, self.modules.len(), -1),
            KeyCode::Down | KeyCode::Char('j') => move_selection(&mut self.module_sel, self.modules.len(), 1),
            KeyCode::Char('e') | KeyCode::Char(' ') | KeyCode::Enter => {
                return self
                    .selected_module()
                    .map(|row| Command::ToggleEnable(row.id.clone()));
            }
            KeyCode::Char('o') => {
                let id = self.selected_module()?.id.clone();
                match self.option_specs().iter().position(|s| s.module == id) {
                    Some(i) => {
                        self.option_sel = i;
                        self.screen = Screen::Options;
                    }
                    None => self.status = format!("{id} não tem opções configuráveis"),
                }
            }
            KeyCode::Char('p') => {
                let row = self.selected_module()?;
                self.editor = Some(Editor::Input {
                    target: EditTarget::Pin(row.id.clone()),
                    buffer: row.pin.clone().unwrap_or_default(),
                    error: None,
                });
            }
            _ => {}
        }
        None
    }

    fn on_conflicts_key(&mut self, code: KeyCode) -> Option<Command> {
        match code {
            KeyCode::Up | KeyCode::Char('k') => move_selection(&mut self.conflict_sel, self.conflicts.len(), -1),
            KeyCode::Down | KeyCode::Char('j') => move_selection(&mut self.conflict_sel, self.conflicts.len(), 1),
            KeyCode::PageUp | KeyCode::Char('u') => self.diff_scroll = self.diff_scroll.saturating_sub(10),
            KeyCode::PageDown | KeyCode::Char('d') => self.diff_scroll = self.diff_scroll.saturating_add(10),
            KeyCode::Char('m') | KeyCode::Char('n') => {
                let resolution = if code == KeyCode::Char('m') {
                    Resolution::KeepMine
                } else {
                    Resolution::UseNew
                };
                return self.selected_conflict().map(|row| Command::Resolve {
                    path: row.path.clone(),
                    resolution,
                });
            }
            _ => {}
        }
        None
    }

    fn on_wizard_key(&mut self, code: KeyCode, mods: KeyModifiers) -> Option<Command> {
        if code == KeyCode::Char('q') {
            self.should_quit = true;
            return None;
        }
        let out = self.wizard.as_mut()?.on_key(code, mods)?;
        match out {
            WizardOut::Close => {
                let first = self.wizard.as_ref().is_some_and(|w| w.first_run);
                self.wizard = None;
                self.status = if first {
                    "assistente fechado sem gravar — w reabre".to_string()
                } else {
                    "assistente fechado, nada mudou".to_string()
                };
                None
            }
            WizardOut::Finish { settings, every } => {
                self.wizard = None;
                Some(Command::FinishWizard { settings, every })
            }
            WizardOut::OptionKey(code, mods) => self.on_option_key(code, mods),
        }
    }

    /// Teclas do painel de opções, compartilhado entre a tela Opções e o assistente.
    fn on_option_key(&mut self, code: KeyCode, mods: KeyModifiers) -> Option<Command> {
        let in_wizard = self.wizard.is_some();
        let specs = match &self.wizard {
            Some(w) => w.option_specs(),
            None => self.option_specs(),
        };
        let len = specs.len();
        let sel = match self.wizard.as_mut() {
            Some(w) => &mut w.option_sel,
            None => &mut self.option_sel,
        };
        match code {
            KeyCode::Up | KeyCode::Char('k') => {
                move_selection(sel, len, -1);
                return None;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                move_selection(sel, len, 1);
                return None;
            }
            KeyCode::Char('a') if !in_wizard => {
                self.live = !self.live;
                self.status = if self.live {
                    "aplicar ao vivo: ligado (cada mudança roda um sync só do módulo)".to_string()
                } else {
                    "aplicar ao vivo: desligado (s aplica tudo)".to_string()
                };
                return None;
            }
            _ => {}
        }
        let spec = specs.get(*sel)?.clone();
        let current = match &self.wizard {
            Some(w) => spec.value_in(&w.draft).0,
            None => spec.value_in(&self.settings).0,
        };
        let shift = mods.contains(KeyModifiers::SHIFT);
        let big = |small: i64| if shift { small * 10 } else { small };
        let delta = match code {
            KeyCode::Left | KeyCode::Char('h') => Some(big(-1)),
            KeyCode::Right | KeyCode::Char('l') => Some(big(1)),
            KeyCode::Char('H') => Some(-10),
            KeyCode::Char('L') => Some(10),
            _ => None,
        };
        if let Some(delta) = delta {
            match spec.kind {
                OptKind::Text | OptKind::Color => {
                    self.status = "Enter para editar".to_string();
                    return None;
                }
                OptKind::Lines | OptKind::Keymaps => {
                    self.status = format!("{}: edite no app (Hiraku Setup Manager) ou com `hiraku set`", spec.key);
                    return None;
                }
                _ => {}
            }
            let next = spec.step(&current, delta);
            return self.commit_option(&spec, Some(next));
        }
        match code {
            KeyCode::Enter | KeyCode::Char(' ') => match &spec.kind {
                OptKind::Choice(values) => {
                    let sel = values.iter().position(|v| *v == current).unwrap_or(0);
                    self.editor = Some(Editor::Pick {
                        values: values.clone(),
                        spec,
                        sel,
                    });
                    None
                }
                OptKind::Bool => {
                    let next = spec.step(&current, 1);
                    self.commit_option(&spec, Some(next))
                }
                OptKind::Lines | OptKind::Keymaps => {
                    self.status = format!("{}: edite no app (Hiraku Setup Manager) ou com `hiraku set`", spec.key);
                    None
                }
                OptKind::Number { .. } | OptKind::Text | OptKind::Color => {
                    self.editor = Some(Editor::Input {
                        target: EditTarget::Option(spec),
                        buffer: current,
                        error: None,
                    });
                    None
                }
            },
            KeyCode::Char('d') | KeyCode::Backspace | KeyCode::Delete => self.commit_option(&spec, None),
            _ => None,
        }
    }

    /// Grava uma opção: no rascunho do assistente ou, no hub, via `Command`.
    fn commit_option(&mut self, spec: &OptSpec, value: Option<String>) -> Option<Command> {
        // Valor igual ao padrão é guardado como "sem escolha" (settings.toml limpo).
        let value = value.filter(|v| *v != spec.default);
        if let Some(w) = self.wizard.as_mut() {
            match &value {
                Some(v) => w.draft.set_option(&spec.module, &spec.key, v),
                None => remove_option(&mut w.draft, &spec.module, &spec.key),
            }
            return None;
        }
        let current = self.settings.options.get(&spec.module).and_then(|o| o.get(&spec.key));
        if current == value.as_ref() {
            return None;
        }
        Some(Command::SetOption {
            module: spec.module.clone(),
            key: spec.key.clone(),
            value,
        })
    }

    fn on_editor_key(&mut self, code: KeyCode, mods: KeyModifiers) -> Option<Command> {
        let editor = self.editor.as_mut()?;
        match editor {
            Editor::Pick { spec, values, sel } => match code {
                KeyCode::Esc | KeyCode::Char('q') => self.editor = None,
                KeyCode::Up | KeyCode::Char('k') => move_selection(sel, values.len(), -1),
                KeyCode::Down | KeyCode::Char('j') => move_selection(sel, values.len(), 1),
                KeyCode::PageUp => move_selection(sel, values.len(), -10),
                KeyCode::PageDown => move_selection(sel, values.len(), 10),
                KeyCode::Home => *sel = 0,
                KeyCode::End => *sel = values.len().saturating_sub(1),
                KeyCode::Enter | KeyCode::Char(' ') => {
                    let spec = spec.clone();
                    let value = values.get(*sel).cloned();
                    self.editor = None;
                    return self.commit_option(&spec, value);
                }
                _ => {}
            },
            Editor::Input { target, buffer, error } => match code {
                KeyCode::Esc => self.editor = None,
                KeyCode::Backspace => {
                    buffer.pop();
                    *error = None;
                }
                KeyCode::Char(c) if !mods.contains(KeyModifiers::CONTROL) => {
                    buffer.push(c);
                    *error = None;
                }
                KeyCode::Enter => match target.clone() {
                    EditTarget::Option(spec) => match spec.validate(buffer) {
                        Ok(value) => {
                            self.editor = None;
                            return self.commit_option(&spec, Some(value));
                        }
                        Err(e) => *error = Some(e),
                    },
                    EditTarget::Pin(module) => {
                        let pin = buffer.trim().to_string();
                        self.editor = None;
                        return Some(Command::SetPin {
                            module,
                            pin: (!pin.is_empty()).then_some(pin),
                        });
                    }
                },
                _ => {}
            },
        }
        None
    }
}

impl Default for App {
    fn default() -> Self {
        App::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::ModuleInfo;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn char(c: char) -> KeyEvent {
        key(KeyCode::Char(c))
    }

    fn module(id: &str, enabled: bool) -> ModuleRow {
        ModuleRow {
            id: id.to_string(),
            description: format!("módulo {id}"),
            enabled,
            installed: None,
            pin: None,
            supported: true,
        }
    }

    fn app_with(modules: &[&str], conflicts: &[&str]) -> App {
        let mut app = App::new();
        app.apply_snapshot(Snapshot {
            modules: modules.iter().map(|id| module(id, false)).collect(),
            conflicts: conflicts
                .iter()
                .map(|p| ConflictRow {
                    path: p.to_string(),
                    module: "kitty".to_string(),
                })
                .collect(),
            ..Snapshot::default()
        });
        app
    }

    fn kitty_catalog() -> Vec<ModuleInfo> {
        let spec = |key: &str, kind: OptKind, default: &str| OptSpec {
            module: "kitty".into(),
            key: key.into(),
            kind,
            default: default.into(),
            description: format!("desc {key}"),
            ..Default::default()
        };
        vec![
            ModuleInfo {
                id: "kitty".into(),
                name: "kitty".into(),
                description: "terminal".into(),
                supported: true,
                requires: vec![],
                options: vec![
                    spec(
                        "opacity",
                        OptKind::Number {
                            min: 30,
                            max: 100,
                            divisor: 100,
                        },
                        "80",
                    ),
                    spec(
                        "palette",
                        OptKind::Choice(vec!["navy".into(), "nord".into(), "dracula".into()]),
                        "navy",
                    ),
                    spec("title", OptKind::Text, "kitty"),
                ],
                homepage: None,
                discover: Vec::new(),
            },
            ModuleInfo {
                id: "tmux".into(),
                name: "tmux".into(),
                description: "mux".into(),
                supported: true,
                requires: vec![],
                options: vec![],
                homepage: None,
                discover: Vec::new(),
            },
        ]
    }

    fn hub() -> App {
        let mut app = App::new();
        let mut settings = Settings::default();
        settings.enable("kitty");
        app.apply_snapshot(Snapshot {
            modules: vec![
                ModuleRow {
                    installed: Some("0.36.4".into()),
                    ..module("kitty", true)
                },
                module("tmux", false),
            ],
            catalog: kitty_catalog(),
            settings,
            ..Snapshot::default()
        });
        app
    }

    #[test]
    fn tab_cycles_screens_and_numbers_jump() {
        let mut app = App::new();
        assert_eq!(app.screen, Screen::Modules);
        app.on_key(key(KeyCode::Tab));
        assert_eq!(app.screen, Screen::Options);
        app.on_key(key(KeyCode::BackTab));
        app.on_key(key(KeyCode::BackTab));
        assert_eq!(app.screen, Screen::Help);
        app.on_key(char('4'));
        assert_eq!(app.screen, Screen::Report);
        app.on_key(char('3'));
        assert_eq!(app.screen, Screen::Conflicts);
        app.on_key(char('?'));
        assert_eq!(app.screen, Screen::Help);
        app.on_key(char('1'));
        assert_eq!(app.screen, Screen::Modules);
    }

    #[test]
    fn selection_moves_and_clamps() {
        let mut app = app_with(&["a", "b", "c"], &[]);
        app.on_key(key(KeyCode::Up));
        assert_eq!(app.module_sel, 0);
        app.on_key(char('j'));
        app.on_key(key(KeyCode::Down));
        app.on_key(key(KeyCode::Down));
        assert_eq!(app.module_sel, 2);
        app.on_key(char('k'));
        assert_eq!(app.module_sel, 1);
    }

    #[test]
    fn toggle_returns_a_command_for_the_selected_module() {
        let mut app = app_with(&["a", "b"], &[]);
        app.on_key(key(KeyCode::Down));
        assert_eq!(app.on_key(char('e')), Some(Command::ToggleEnable("b".to_string())));
        assert_eq!(app.on_key(char(' ')), Some(Command::ToggleEnable("b".to_string())));
        assert_eq!(app.on_key(key(KeyCode::Enter)), Some(Command::ToggleEnable("b".to_string())));
        assert_eq!(App::new().on_key(char('e')), None);
    }

    #[test]
    fn resolve_keys_work_only_on_the_conflicts_screen() {
        let mut app = app_with(&[], &["/h/a.conf", "/h/b.conf"]);
        assert_eq!(app.on_key(char('m')), None);
        app.on_key(char('3'));
        app.on_key(char('j'));
        assert_eq!(
            app.on_key(char('m')),
            Some(Command::Resolve {
                path: "/h/b.conf".to_string(),
                resolution: Resolution::KeepMine
            })
        );
        assert_eq!(
            app.on_key(char('n')),
            Some(Command::Resolve {
                path: "/h/b.conf".to_string(),
                resolution: Resolution::UseNew
            })
        );
        let mut empty = App::new();
        empty.on_key(char('3'));
        assert_eq!(empty.on_key(char('n')), None);
    }

    #[test]
    fn diff_is_requested_only_when_needed() {
        let mut app = app_with(&[], &["/h/a.conf", "/h/b.conf"]);
        assert_eq!(app.needs_diff(), None, "fora da tela de conflitos");
        app.on_key(char('3'));
        assert_eq!(app.needs_diff().as_deref(), Some("/h/a.conf"));
        app.set_diff("/h/a.conf".to_string(), Ok("+nova\n".to_string()));
        assert_eq!(app.needs_diff(), None);
        app.on_key(char('j'));
        assert_eq!(app.needs_diff().as_deref(), Some("/h/b.conf"));
        app.set_diff("/h/b.conf".to_string(), Err("binário".to_string()));
        assert!(app.diff_text.contains("binário"));
        app.set_diff("/h/b.conf".to_string(), Ok("  \n".to_string()));
        assert_eq!(app.diff_text, "(sem diferenças)");
    }

    #[test]
    fn diff_scrolls_without_underflow() {
        let mut app = app_with(&[], &["/h/a.conf"]);
        app.on_key(char('3'));
        app.on_key(char('u'));
        assert_eq!(app.diff_scroll, 0);
        app.on_key(char('d'));
        app.on_key(key(KeyCode::PageDown));
        assert_eq!(app.diff_scroll, 20);
        app.on_key(key(KeyCode::PageUp));
        assert_eq!(app.diff_scroll, 10);
    }

    #[test]
    fn snapshot_clamps_selection_and_resets_diff() {
        let mut app = app_with(&["a", "b", "c"], &["/h/a", "/h/b"]);
        app.module_sel = 2;
        app.conflict_sel = 1;
        app.set_diff("/h/b".to_string(), Ok("x".to_string()));
        app.apply_snapshot(Snapshot {
            modules: vec![module("a", true)],
            ..Snapshot::default()
        });
        assert_eq!(app.module_sel, 0);
        assert_eq!(app.conflict_sel, 0);
        assert!(app.diff_path.is_none());
        assert!(app.diff_text.is_empty());
    }

    #[test]
    fn quit_rules() {
        let mut app = App::new();
        app.on_key(char('q'));
        assert!(app.should_quit);

        let mut busy = App::new();
        busy.running = true;
        busy.on_key(char('q'));
        assert!(!busy.should_quit, "q não interrompe um sync em andamento");
        assert!(busy.status.contains("Ctrl-C"));
        busy.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(busy.should_quit, "Ctrl-C sempre sai");
    }

    #[test]
    fn sync_and_reload_commands() {
        let mut app = App::new();
        assert_eq!(app.on_key(char('s')), Some(Command::StartSync));
        assert_eq!(app.on_key(char('r')), Some(Command::Reload));
        app.running = true;
        assert_eq!(app.on_key(char('s')), None);
        assert!(app.status.contains("já está"));
    }

    #[test]
    fn log_is_capped() {
        let mut app = App::new();
        for i in 0..(MAX_LOG_LINES + 50) {
            app.push_log(format!("linha {i}"));
        }
        assert_eq!(app.log.len(), MAX_LOG_LINES);
        assert_eq!(app.log[0], "linha 50");
    }

    #[test]
    fn finish_sync_updates_status() {
        let mut app = App::new();
        app.running = true;
        app.finish_sync(Err("boom".to_string()));
        assert!(!app.running);
        assert!(app.status.contains("boom"));
    }

    // ---- tela Opções ----

    #[test]
    fn options_are_ordered_choices_first() {
        let app = hub();
        let keys: Vec<String> = app.option_specs().iter().map(|s| s.key.clone()).collect();
        assert_eq!(keys, vec!["palette", "opacity", "title"]);
    }

    #[test]
    fn arrows_cycle_and_emit_set_option() {
        let mut app = hub();
        app.on_key(char('2'));
        assert_eq!(
            app.on_key(key(KeyCode::Right)),
            Some(Command::SetOption {
                module: "kitty".into(),
                key: "palette".into(),
                value: Some("nord".into())
            })
        );
        app.on_key(char('j'));
        assert_eq!(
            app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT)),
            Some(Command::SetOption {
                module: "kitty".into(),
                key: "opacity".into(),
                value: Some("70".into())
            })
        );
        // Voltar ao padrão grava "sem escolha".
        app.settings.set_option("kitty", "opacity", "81");
        assert_eq!(
            app.on_key(char('h')),
            Some(Command::SetOption {
                module: "kitty".into(),
                key: "opacity".into(),
                value: None
            })
        );
        assert_eq!(
            app.on_key(char('d')),
            Some(Command::SetOption {
                module: "kitty".into(),
                key: "opacity".into(),
                value: None
            })
        );
    }

    #[test]
    fn enter_opens_picker_and_picking_commits() {
        let mut app = hub();
        app.on_key(char('2'));
        app.on_key(key(KeyCode::Enter));
        assert!(matches!(app.editor, Some(Editor::Pick { sel: 0, .. })));
        app.on_key(char('j'));
        app.on_key(char('j'));
        let (_, value) = app.focused_choice().unwrap();
        assert_eq!(value, "dracula", "a pré-visualização segue o item destacado");
        assert_eq!(
            app.on_key(key(KeyCode::Enter)),
            Some(Command::SetOption {
                module: "kitty".into(),
                key: "palette".into(),
                value: Some("dracula".into())
            })
        );
        assert!(app.editor.is_none());
        // Esc cancela.
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.on_key(key(KeyCode::Esc)), None);
        assert!(app.editor.is_none());
    }

    #[test]
    fn number_input_validates_before_committing() {
        let mut app = hub();
        app.on_key(char('2'));
        app.on_key(char('j'));
        app.on_key(key(KeyCode::Enter));
        for _ in 0..2 {
            app.on_key(key(KeyCode::Backspace));
        }
        app.on_key(char('9'));
        app.on_key(char('9'));
        app.on_key(char('9'));
        assert_eq!(app.on_key(key(KeyCode::Enter)), None);
        assert!(matches!(&app.editor, Some(Editor::Input { error: Some(e), .. }) if e.contains("fora")));
        app.on_key(key(KeyCode::Backspace));
        assert_eq!(
            app.on_key(key(KeyCode::Enter)),
            Some(Command::SetOption {
                module: "kitty".into(),
                key: "opacity".into(),
                value: Some("99".into())
            })
        );
    }

    #[test]
    fn editor_swallows_global_keys() {
        let mut app = hub();
        app.on_key(char('2'));
        app.on_key(char('j'));
        app.on_key(char('j'));
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.on_key(char('q')), None);
        assert!(!app.should_quit, "q é texto dentro do campo");
        assert_eq!(app.on_key(char('s')), None, "s também");
    }

    #[test]
    fn modules_screen_jumps_to_options_and_edits_pins() {
        let mut app = hub();
        app.on_key(char('o'));
        assert_eq!(app.screen, Screen::Options);
        assert_eq!(app.option_specs()[app.option_sel].module, "kitty");

        app.on_key(char('1'));
        app.on_key(char('j'));
        app.on_key(char('o'));
        assert_eq!(app.screen, Screen::Modules);
        assert!(app.status.contains("tmux não tem opções"));

        app.on_key(char('p'));
        for c in "3.*".chars() {
            app.on_key(char(c));
        }
        assert_eq!(
            app.on_key(key(KeyCode::Enter)),
            Some(Command::SetPin {
                module: "tmux".into(),
                pin: Some("3.*".into())
            })
        );
        app.on_key(char('p'));
        assert_eq!(
            app.on_key(key(KeyCode::Enter)),
            Some(Command::SetPin {
                module: "tmux".into(),
                pin: None
            })
        );
    }

    #[test]
    fn preview_is_requested_once_per_value() {
        let mut app = hub();
        assert_eq!(app.preview_request(), None, "fora da tela de opções");
        app.on_key(char('2'));
        assert_eq!(
            app.preview_request(),
            Some(("kitty".into(), "palette".into(), "navy".into()))
        );
        app.set_preview("kitty".into(), "palette".into(), "navy".into(), vec![("background".into(), (1, 2, 3))]);
        assert_eq!(app.preview_request(), None);
        assert_eq!(app.current_preview().unwrap().color("background"), Some((1, 2, 3)));
        app.on_key(char('j'));
        assert_eq!(app.preview_request(), None, "número não tem pré-visualização");
        assert!(app.current_preview().is_none());
    }

    #[test]
    fn live_apply_is_debounced_and_only_for_installed_enabled_modules() {
        let mut app = hub();
        let t0 = Instant::now();
        app.queue_apply("kitty", t0);
        assert_eq!(app.due_apply(t0), None);
        assert_eq!(app.due_apply(t0 + APPLY_DELAY), Some("kitty".to_string()));
        assert_eq!(app.due_apply(t0 + APPLY_DELAY * 2), None);

        app.queue_apply("tmux", t0);
        assert!(app.pending_apply.is_none());
        assert!(app.status.contains("desabilitado"));

        app.on_key(char('2'));
        app.on_key(char('a'));
        assert!(!app.live);
        app.queue_apply("kitty", t0);
        assert!(app.pending_apply.is_none());

        app.live = true;
        app.queue_apply("kitty", t0);
        app.running = true;
        assert_eq!(app.due_apply(t0 + APPLY_DELAY), None, "espera o sync atual");
        app.running = false;
        app.on_key(char('s'));
        assert!(app.pending_apply.is_none(), "s aplica tudo e descarta o pendente");
    }

    // ---- assistente ----

    #[test]
    fn wizard_edits_a_draft_and_finishes_with_a_command() {
        let mut app = hub();
        app.on_key(char('w'));
        assert!(app.wizard.is_some());
        assert_eq!(app.on_key(char('s')), None, "teclas globais ficam presas no assistente");
        app.on_key(key(KeyCode::Enter)); // modo
        app.on_key(key(KeyCode::Enter)); // editor
        app.on_key(key(KeyCode::Enter)); // módulos
        assert_eq!(app.wizard.as_ref().unwrap().step, Step::Options);
        assert_eq!(app.on_key(key(KeyCode::Right)), None, "no assistente vai pro rascunho");
        assert_eq!(app.wizard.as_ref().unwrap().draft.options["kitty"]["palette"], "nord");
        assert!(app.settings.options.is_empty(), "nada gravado ainda");
        app.on_key(key(KeyCode::Tab)); // atualizações
        app.on_key(key(KeyCode::Enter)); // resumo
        match app.on_key(key(KeyCode::Enter)) {
            Some(Command::FinishWizard { settings, .. }) => {
                assert_eq!(settings.options["kitty"]["palette"], "nord");
            }
            other => panic!("esperava FinishWizard, veio {other:?}"),
        }
        assert!(app.wizard.is_none());
    }

    #[test]
    fn wizard_esc_closes_without_saving() {
        let mut app = hub();
        app.start_wizard(true);
        assert_eq!(app.on_key(key(KeyCode::Esc)), None);
        assert!(app.wizard.is_none());
        assert!(app.status.contains("w reabre"));
    }
}
