//! Assistente de instalação (primeira execução, ou `w` no hub para refazer).
//!
//! Faz as mesmas perguntas do antigo `install.sh` — sudo?, qual editor, o que
//! instalar, aparência — e mais uma: com que frequência buscar atualizações.
//! Trabalha num rascunho de `Settings`; só grava quando o usuário confirma.
//! Puro: devolve `WizardOut` e quem grava/roda o sync é o laço da TUI.

use hiraku_core::settings::{Mode, Settings};
use ratatui::crossterm::event::{KeyCode, KeyModifiers};

pub use hiraku_hub::presets::{
    EDITOR_CHOICES, MODE_CHOICES, ONE_DAY, RECOMMENDED, SIX_HOURS, UPDATE_CHOICES,
};

use crate::catalog::{with_dependencies, ModuleInfo, OptSpec};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Mode,
    Editor,
    Modules,
    Options,
    Updates,
    Confirm,
}

impl Step {
    pub const ALL: [Step; 6] = [
        Step::Mode,
        Step::Editor,
        Step::Modules,
        Step::Options,
        Step::Updates,
        Step::Confirm,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Step::Mode => "Permissões",
            Step::Editor => "Editor",
            Step::Modules => "O que instalar",
            Step::Options => "Aparência e opções",
            Step::Updates => "Atualizações",
            Step::Confirm => "Revisar e instalar",
        }
    }

    pub fn index(self) -> usize {
        Step::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum WizardOut {
    /// Sai do assistente sem gravar nada.
    Close,
    /// Grava as configurações, ajusta o agendamento e roda o primeiro sync.
    Finish {
        settings: Settings,
        every: Option<u64>,
    },
    /// Na etapa de opções: tecla repassada para o painel de opções compartilhado.
    OptionKey(KeyCode, KeyModifiers),
}

#[derive(Debug, Clone)]
pub struct Wizard {
    pub step: Step,
    pub draft: Settings,
    /// Primeira execução (não havia settings.toml).
    pub first_run: bool,
    pub can_root: bool,
    pub mode_sel: usize,
    pub editor_sel: usize,
    pub module_sel: usize,
    pub option_sel: usize,
    pub update_sel: usize,
    /// Módulos suportados neste sistema, na ordem de exibição.
    pub choices: Vec<ModuleInfo>,
}

impl Wizard {
    /// `current` é o settings.toml atual (vazio na primeira execução).
    pub fn new(
        catalog: &[ModuleInfo],
        current: &Settings,
        first_run: bool,
        can_root: bool,
        scheduled: bool,
    ) -> Wizard {
        let choices: Vec<ModuleInfo> = catalog.iter().filter(|m| m.supported).cloned().collect();
        let mut draft = current.clone();
        if first_run {
            draft.modules = choices
                .iter()
                .filter(|m| RECOMMENDED.contains(&m.id.as_str()))
                .map(|m| m.id.clone())
                .collect();
        }
        let has = |id: &str| draft.modules.iter().any(|m| m == id);
        let editor_sel = match (has("vim"), has("nvim")) {
            (true, true) => 0,
            (false, true) => 1,
            (true, false) => 2,
            (false, false) if first_run => 0,
            (false, false) => 3,
        };
        let mode_sel = match draft.mode {
            None => 0,
            Some(Mode::User) => 1,
            Some(Mode::System) => 2,
        };
        Wizard {
            step: Step::Mode,
            draft,
            first_run,
            can_root,
            mode_sel,
            editor_sel,
            module_sel: 0,
            option_sel: 0,
            update_sel: if first_run || scheduled { 1 } else { 2 },
            choices,
        }
    }

    pub fn is_enabled(&self, id: &str) -> bool {
        self.draft.modules.iter().any(|m| m == id)
    }

    /// Opções dos módulos escolhidos (e das dependências deles).
    pub fn option_specs(&self) -> Vec<OptSpec> {
        let ids = with_dependencies(&self.choices, &self.draft.modules);
        sorted_specs(self.choices.iter().filter(|m| ids.contains(&m.id)))
    }

    pub fn every(&self) -> Option<u64> {
        UPDATE_CHOICES[self.update_sel].1
    }

    fn list_len(&self) -> usize {
        match self.step {
            Step::Mode => MODE_CHOICES.len(),
            Step::Editor => EDITOR_CHOICES.len(),
            Step::Modules => self.choices.len(),
            Step::Options => self.option_specs().len(),
            Step::Updates => UPDATE_CHOICES.len(),
            Step::Confirm => 0,
        }
    }

    fn selection(&mut self) -> Option<&mut usize> {
        match self.step {
            Step::Mode => Some(&mut self.mode_sel),
            Step::Editor => Some(&mut self.editor_sel),
            Step::Modules => Some(&mut self.module_sel),
            Step::Options => Some(&mut self.option_sel),
            Step::Updates => Some(&mut self.update_sel),
            Step::Confirm => None,
        }
    }

    /// Aplica a escolha da etapa atual ao rascunho.
    fn commit_step(&mut self) {
        match self.step {
            Step::Mode => {
                self.draft.mode = match self.mode_sel {
                    1 => Some(Mode::User),
                    2 => Some(Mode::System),
                    _ => None,
                };
            }
            Step::Editor => {
                let wanted = EDITOR_CHOICES[self.editor_sel].1;
                for id in ["vim", "nvim"] {
                    let available = self.choices.iter().any(|m| m.id == id);
                    if wanted.contains(&id) && available {
                        self.draft.enable(id);
                    } else {
                        self.draft.disable(id);
                    }
                }
            }
            _ => {}
        }
    }

    fn next(&mut self) {
        self.commit_step();
        let mut i = self.step.index() + 1;
        // Pula a etapa de opções se nada escolhido tiver opções.
        if Step::ALL.get(i) == Some(&Step::Options) && self.option_specs().is_empty() {
            i += 1;
        }
        if let Some(step) = Step::ALL.get(i) {
            self.step = *step;
            self.option_sel = self.option_sel.min(self.option_specs().len().saturating_sub(1));
        }
    }

    /// Volta uma etapa; `false` se já estava na primeira.
    fn back(&mut self) -> bool {
        let mut i = self.step.index();
        if i == 0 {
            return false;
        }
        i -= 1;
        if Step::ALL[i] == Step::Options && self.option_specs().is_empty() {
            i -= 1;
        }
        self.step = Step::ALL[i];
        true
    }

    pub fn on_key(&mut self, code: KeyCode, mods: KeyModifiers) -> Option<WizardOut> {
        // Navegação entre etapas: igual em todas.
        match code {
            KeyCode::Tab => {
                if self.step == Step::Confirm {
                    return None;
                }
                self.next();
                return None;
            }
            KeyCode::BackTab | KeyCode::Esc => {
                if !self.back() && code == KeyCode::Esc {
                    return Some(WizardOut::Close);
                }
                return None;
            }
            _ => {}
        }

        if self.step == Step::Options {
            return Some(WizardOut::OptionKey(code, mods));
        }

        let len = self.list_len();
        match code {
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(sel) = self.selection() {
                    *sel = sel.saturating_sub(1);
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(sel) = self.selection() {
                    *sel = (*sel + 1).min(len.saturating_sub(1));
                }
            }
            KeyCode::Char(' ') if self.step == Step::Modules => self.toggle_selected_module(),
            KeyCode::Char('a') if self.step == Step::Modules => {
                // Marca/desmarca tudo.
                let all = self.choices.iter().all(|m| self.is_enabled(&m.id));
                let ids: Vec<String> = self.choices.iter().map(|m| m.id.clone()).collect();
                for id in ids {
                    if all {
                        self.draft.disable(&id);
                    } else {
                        self.draft.enable(&id);
                    }
                }
            }
            KeyCode::Enter => {
                if self.step == Step::Confirm {
                    if self.draft.modules.is_empty() {
                        return None;
                    }
                    return Some(WizardOut::Finish {
                        settings: self.draft.clone(),
                        every: self.every(),
                    });
                }
                self.next();
            }
            _ => {}
        }
        None
    }

    fn toggle_selected_module(&mut self) {
        let Some(id) = self.choices.get(self.module_sel).map(|m| m.id.clone()) else {
            return;
        };
        if !self.draft.disable(&id) {
            self.draft.enable(&id);
        }
        // Mantém a escolha do editor coerente com a lista.
        self.editor_sel = match (self.is_enabled("vim"), self.is_enabled("nvim")) {
            (true, true) => 0,
            (false, true) => 1,
            (true, false) => 2,
            (false, false) => 3,
        };
    }

    /// Linhas do resumo final.
    pub fn summary(&self) -> Vec<(String, String)> {
        let mode = MODE_CHOICES[match self.draft.mode {
            None => 0,
            Some(Mode::User) => 1,
            Some(Mode::System) => 2,
        }]
        .0;
        let all = with_dependencies(&self.choices, &self.draft.modules);
        let extra: Vec<&String> = all.iter().filter(|id| !self.is_enabled(id)).collect();
        let mut modules = self.draft.modules.join(", ");
        if !extra.is_empty() {
            modules.push_str(&format!(
                "  (+ dependências: {})",
                extra.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
            ));
        }
        let mut lines = vec![
            ("Modo".to_string(), mode.to_string()),
            (
                "Módulos".to_string(),
                if modules.is_empty() {
                    "(nenhum — volte e escolha algo)".to_string()
                } else {
                    modules
                },
            ),
        ];
        for spec in self.option_specs() {
            let (value, custom) = spec.value_in(&self.draft);
            if custom {
                lines.push((format!("{}.{}", spec.module, spec.key), spec.display(&value)));
            }
        }
        lines.push((
            "Atualizações".to_string(),
            UPDATE_CHOICES[self.update_sel].0.to_string(),
        ));
        lines
    }
}

pub use crate::catalog::sorted_specs;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::OptKind;

    fn info(id: &str, req: &[&str], options: Vec<OptSpec>) -> ModuleInfo {
        ModuleInfo {
            id: id.to_string(),
            name: id.to_string(),
            description: format!("desc {id}"),
            supported: true,
            requires: req.iter().map(|s| s.to_string()).collect(),
            options,
        }
    }

    fn palette() -> OptSpec {
        OptSpec {
            module: "kitty".into(),
            key: "palette".into(),
            kind: OptKind::Choice(vec!["navy".into(), "nord".into()]),
            default: "navy".into(),
            description: String::new(),
        }
    }

    fn catalog() -> Vec<ModuleInfo> {
        let mut unsupported = info("winonly", &[], vec![]);
        unsupported.supported = false;
        vec![
            info("font-jetbrains-mono", &[], vec![]),
            info("kitty", &["font-jetbrains-mono"], vec![palette()]),
            info("nvim", &["font-jetbrains-mono"], vec![]),
            info("tmux", &[], vec![]),
            info("vim", &[], vec![]),
            unsupported,
        ]
    }

    fn press(w: &mut Wizard, code: KeyCode) -> Option<WizardOut> {
        w.on_key(code, KeyModifiers::NONE)
    }

    #[test]
    fn first_run_preselects_the_recommended_set() {
        let w = Wizard::new(&catalog(), &Settings::default(), true, true, false);
        assert!(w.is_enabled("kitty") && w.is_enabled("vim") && w.is_enabled("nvim"));
        assert!(!w.choices.iter().any(|m| m.id == "winonly"), "esconde o que não roda aqui");
        assert_eq!(w.editor_sel, 0);
        assert_eq!(w.update_sel, 1, "padrão: uma vez por dia");
    }

    #[test]
    fn rerun_starts_from_the_current_settings() {
        let mut current = Settings {
            mode: Some(Mode::User),
            ..Settings::default()
        };
        current.enable("nvim");
        let w = Wizard::new(&catalog(), &current, false, true, false);
        assert_eq!(w.mode_sel, 1);
        assert_eq!(w.editor_sel, 1);
        assert_eq!(w.draft.modules, vec!["nvim"]);
        assert_eq!(w.update_sel, 2, "sem agendamento: manual");
    }

    #[test]
    fn full_walkthrough_produces_settings() {
        let mut w = Wizard::new(&catalog(), &Settings::default(), true, true, false);
        // Modo: "só no usuário".
        press(&mut w, KeyCode::Down);
        press(&mut w, KeyCode::Enter);
        assert_eq!(w.step, Step::Editor);
        // Editor: só Neovim.
        press(&mut w, KeyCode::Down);
        press(&mut w, KeyCode::Enter);
        assert!(w.is_enabled("nvim") && !w.is_enabled("vim"));
        assert_eq!(w.step, Step::Modules);
        // Desmarca o tmux.
        let tmux = w.choices.iter().position(|m| m.id == "tmux").unwrap();
        w.module_sel = tmux;
        press(&mut w, KeyCode::Char(' '));
        assert!(!w.is_enabled("tmux"));
        press(&mut w, KeyCode::Enter);
        assert_eq!(w.step, Step::Options, "kitty tem opções");
        // Na etapa de opções as teclas vão para o painel compartilhado.
        assert_eq!(
            press(&mut w, KeyCode::Right),
            Some(WizardOut::OptionKey(KeyCode::Right, KeyModifiers::NONE))
        );
        w.draft.set_option("kitty", "palette", "nord");
        press(&mut w, KeyCode::Tab);
        assert_eq!(w.step, Step::Updates);
        press(&mut w, KeyCode::Up);
        press(&mut w, KeyCode::Enter);
        assert_eq!(w.step, Step::Confirm);

        let summary = w.summary();
        assert!(summary.iter().any(|(k, v)| k == "Modo" && v.contains("sem sudo")));
        assert!(summary.iter().any(|(k, v)| k == "kitty.palette" && v == "nord"));
        assert!(summary.iter().any(|(_, v)| v.contains("6 horas")));

        match press(&mut w, KeyCode::Enter) {
            Some(WizardOut::Finish { settings, every }) => {
                assert_eq!(settings.mode, Some(Mode::User));
                assert!(settings.modules.contains(&"nvim".to_string()));
                assert!(!settings.modules.contains(&"tmux".to_string()));
                assert_eq!(settings.options["kitty"]["palette"], "nord");
                assert_eq!(every, Some(SIX_HOURS));
            }
            other => panic!("esperava Finish, veio {other:?}"),
        }
    }

    #[test]
    fn options_step_is_skipped_when_nothing_has_options() {
        let mut w = Wizard::new(&catalog(), &Settings::default(), true, true, false);
        w.draft.modules = vec!["tmux".to_string()];
        w.step = Step::Modules;
        press(&mut w, KeyCode::Enter);
        assert_eq!(w.step, Step::Updates);
        press(&mut w, KeyCode::BackTab);
        assert_eq!(w.step, Step::Modules, "voltar também pula");
    }

    #[test]
    fn dependency_options_are_offered() {
        let mut w = Wizard::new(&catalog(), &Settings::default(), true, true, false);
        w.draft.modules = vec!["nvim".to_string()];
        assert!(w.option_specs().is_empty());
        w.draft.modules = vec!["kitty".to_string()];
        assert_eq!(w.option_specs().len(), 1);
        let summary = w.summary();
        assert!(summary.iter().any(|(_, v)| v.contains("dependências: font-jetbrains-mono")));
    }

    #[test]
    fn esc_on_first_step_closes_and_empty_selection_cannot_finish() {
        let mut w = Wizard::new(&catalog(), &Settings::default(), true, true, false);
        assert_eq!(press(&mut w, KeyCode::Esc), Some(WizardOut::Close));
        w.draft.modules.clear();
        w.step = Step::Confirm;
        assert_eq!(press(&mut w, KeyCode::Enter), None);
    }

    #[test]
    fn select_all_toggles_everything() {
        let mut w = Wizard::new(&catalog(), &Settings::default(), true, true, false);
        w.step = Step::Modules;
        assert_eq!(w.draft.modules.len(), w.choices.len(), "tudo recomendado já vem marcado");
        press(&mut w, KeyCode::Char('a'));
        assert!(w.draft.modules.is_empty());
        w.module_sel = 0;
        press(&mut w, KeyCode::Char(' '));
        press(&mut w, KeyCode::Char('a'));
        assert_eq!(w.draft.modules.len(), w.choices.len(), "com parte marcada, marca tudo");
    }
}
