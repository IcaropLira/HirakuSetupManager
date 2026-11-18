//! Desenho das telas. Só lê o `App`; nunca altera estado.

use hiraku_core::settings::Settings;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Tabs, Wrap};
use ratatui::Frame;

use crate::app::{report_lines, App, EditTarget, Editor, Preview, Screen};
use crate::catalog::{OptKind, OptSpec};
use crate::wizard::{Step, Wizard, EDITOR_CHOICES, MODE_CHOICES, UPDATE_CHOICES};

const ACCENT: Color = Color::Cyan;

fn dim() -> Style {
    Style::default().fg(Color::DarkGray)
}

fn bold() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

fn highlight() -> Style {
    Style::default()
        .bg(Color::Indexed(238))
        .add_modifier(Modifier::BOLD)
}

pub fn draw(frame: &mut Frame, app: &App) {
    if let Some(wizard) = &app.wizard {
        draw_wizard(frame, app, wizard, frame.area());
    } else {
        draw_hub(frame, app);
    }
    if let Some(editor) = &app.editor {
        draw_editor(frame, app, editor);
    }
}

fn draw_hub(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(8),
        ])
        .split(frame.area());

    draw_tabs(frame, app, chunks[0]);
    match app.screen {
        Screen::Modules => draw_modules(frame, app, chunks[1]),
        Screen::Options => draw_options_panel(
            frame,
            app,
            chunks[1],
            &app.option_specs(),
            &app.settings,
            app.option_sel,
            true,
        ),
        Screen::Conflicts => draw_conflicts(frame, app, chunks[1]),
        Screen::Report => draw_report(frame, app, chunks[1]),
        Screen::Help => draw_help(frame, chunks[1]),
    }
    draw_log(frame, app, chunks[2]);
}

fn draw_tabs(frame: &mut Frame, app: &App, area: Rect) {
    let titles = vec![
        Line::from("1 Módulos"),
        Line::from("2 Opções"),
        Line::from(format!("3 Conflitos ({})", app.conflicts.len())),
        Line::from("4 Relatório"),
        Line::from("5 Ajuda"),
    ];
    let updates = if app.scheduled {
        " atualização automática: ligada "
    } else {
        " atualização automática: desligada (w) "
    };
    let tabs = Tabs::new(titles)
        .select(app.screen.index())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" hiraku · hub de configuração ")
                .title_bottom(Line::from(Span::styled(updates, dim())).right_aligned()),
        )
        .highlight_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD));
    frame.render_widget(tabs, area);
}

fn draw_modules(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Módulos ")
        .title_bottom(Line::from(Span::styled(
            " e habilitar · o opções · p fixar versão · s sincronizar ",
            dim(),
        )));
    if app.modules.is_empty() {
        frame.render_widget(Paragraph::new("Nenhum módulo no registry.").block(block), area);
        return;
    }

    let items: Vec<ListItem> = app
        .modules
        .iter()
        .map(|row| {
            let check = if row.enabled { "[x]" } else { "[ ]" };
            let installed = row.installed.as_deref().unwrap_or("-");
            let pin = row
                .pin
                .as_deref()
                .map(|p| format!(" pin {p}"))
                .unwrap_or_default();
            let text = format!(
                "{check} {:<14} {:<14}{pin}  {}",
                row.id, installed, row.description
            );
            let style = if row.supported { Style::default() } else { dim() };
            ListItem::new(text).style(style)
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(highlight())
        .highlight_symbol("> ");
    let mut state = ListState::default().with_selected(Some(app.module_sel));
    frame.render_stateful_widget(list, area, &mut state);
}

/// Painel de opções: lista agrupada por módulo à esquerda e detalhes (com
/// pré-visualização de cores) à direita. Usado pela tela Opções e pelo assistente.
fn draw_options_panel(
    frame: &mut Frame,
    app: &App,
    area: Rect,
    specs: &[OptSpec],
    values: &Settings,
    sel: usize,
    hub: bool,
) {
    let footer = if hub {
        let live = if app.live { "ligado" } else { "desligado" };
        format!(" ←→ muda · ⇧±10 · Enter edita · d padrão · a ao vivo: {live} ")
    } else {
        " ←→ muda · ⇧±10 · Enter edita · d padrão · Tab segue ".to_string()
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Opções ")
        .title_bottom(Line::from(Span::styled(footer, dim())));
    if specs.is_empty() {
        frame.render_widget(
            Paragraph::new("Nenhum módulo com opções configuráveis.").block(block),
            area,
        );
        return;
    }

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    // Lista com cabeçalhos por módulo; `selected` é o índice já contando cabeçalhos.
    let mut items: Vec<ListItem> = Vec::new();
    let mut selected = 0;
    let mut last_module: Option<&str> = None;
    for (i, spec) in specs.iter().enumerate() {
        if last_module != Some(spec.module.as_str()) {
            let mut header = vec![Span::styled(spec.module.clone(), bold().fg(ACCENT))];
            if hub && !app.is_enabled(&spec.module) {
                header.push(Span::styled("  (desabilitado)", dim()));
            }
            items.push(ListItem::new(Line::from(header)));
            last_module = Some(spec.module.as_str());
        }
        if i == sel {
            selected = items.len();
        }
        let (value, custom) = spec.value_in(values);
        let shown = spec.display(&value);
        let shown = match spec.kind {
            OptKind::Choice(_) | OptKind::Number { .. } => format!("‹ {shown} ›"),
            _ => shown,
        };
        let mut line = vec![
            Span::raw(format!("  {:<16}", spec.key)),
            Span::styled(shown, if custom { bold() } else { Style::default() }),
        ];
        if let Some(gauge) = spec.gauge(&value, 12) {
            line.push(Span::styled(format!("  {gauge}"), Style::default().fg(ACCENT)));
        }
        if custom {
            line.push(Span::styled("  •", Style::default().fg(Color::Yellow)));
        }
        items.push(ListItem::new(Line::from(line)));
    }
    let list = List::new(items)
        .block(block)
        .highlight_style(highlight())
        .highlight_symbol("> ");
    let mut state = ListState::default().with_selected(Some(selected));
    frame.render_stateful_widget(list, cols[0], &mut state);

    // Detalhes da opção selecionada.
    let Some(spec) = specs.get(sel) else {
        return;
    };
    let (value, custom) = spec.value_in(values);
    let mut lines = vec![
        Line::from(Span::styled(format!("{}.{}", spec.module, spec.key), bold())),
        Line::from(""),
    ];
    if !spec.description.is_empty() {
        lines.push(Line::from(spec.description.clone()));
        lines.push(Line::from(""));
    }
    lines.push(Line::from(vec![
        Span::styled("atual   ", dim()),
        Span::raw(spec.display(&value)),
        Span::styled(if custom { "  (sua escolha)" } else { "  (padrão)" }, dim()),
    ]));
    lines.push(Line::from(vec![
        Span::styled("padrão  ", dim()),
        Span::raw(spec.display(&spec.default)),
    ]));
    lines.push(Line::from(vec![Span::styled("valores ", dim()), Span::raw(spec.hint())]));

    let detail_block = Block::default().borders(Borders::ALL).title(" Detalhes ");
    let inner = detail_block.inner(cols[1]);
    frame.render_widget(detail_block, cols[1]);
    let text_height = lines.len() as u16 + 1;
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(text_height), Constraint::Min(0)])
        .split(inner);
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), parts[0]);
    // Com a lista aberta, a pré-visualização aparece dentro dela.
    if app.editor.is_none() {
        if let Some(preview) = app.current_preview() {
            draw_preview(frame, parts[1], preview);
        }
    }
}

fn rgb((r, g, b): (u8, u8, u8)) -> Color {
    Color::Rgb(r, g, b)
}

/// Amostra de terminal com as cores da paleta + as 16 cores ANSI.
fn draw_preview(frame: &mut Frame, area: Rect, preview: &Preview) {
    if area.height < 3 || area.width < 20 {
        return;
    }
    let bg = preview.color("background").map(rgb).unwrap_or(Color::Black);
    let fg = preview.color("foreground").map(rgb).unwrap_or(Color::White);
    let c = |n: usize, fallback: Color| {
        preview
            .color(&format!("color{n}"))
            .map(rgb)
            .unwrap_or(fallback)
    };
    let base = Style::default().bg(bg).fg(fg);
    let width = area.width as usize;
    let pad = |spans: Vec<Span<'static>>| {
        let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
        let mut spans = spans;
        spans.push(Span::styled(" ".repeat(width.saturating_sub(used)), base));
        Line::from(spans)
    };
    let mut lines = vec![
        Line::from(Span::styled(format!("pré-visualização: {}", preview.value), dim())),
        pad(vec![
            Span::styled(" ícaro", base.fg(c(2, Color::Green)).add_modifier(Modifier::BOLD)),
            Span::styled(" in ", base),
            Span::styled("~/projetos", base.fg(c(4, Color::Blue)).add_modifier(Modifier::BOLD)),
            Span::styled(" on ", base),
            Span::styled(" main", base.fg(c(5, Color::Magenta))),
        ]),
        pad(vec![
            Span::styled(" ❯ ", base.fg(c(2, Color::Green))),
            Span::styled("hiraku sync", base),
        ]),
        pad(vec![
            Span::styled(" ✓ ", base.fg(c(2, Color::Green))),
            Span::styled("kitty atualizado ", base),
            Span::styled("⚠ 1 conflito ", base.fg(c(3, Color::Yellow))),
            Span::styled("✗ erro", base.fg(c(1, Color::Red))),
        ]),
    ];
    for row in [0usize, 8] {
        let mut spans = vec![Span::styled(" ", base)];
        for n in row..row + 8 {
            spans.push(Span::styled("██ ", base.fg(c(n, Color::Reset))));
        }
        lines.push(pad(spans));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn diff_line_style(line: &str) -> Style {
    if line.starts_with("+++") || line.starts_with("---") {
        bold()
    } else if line.starts_with('+') {
        Style::default().fg(Color::Green)
    } else if line.starts_with('-') {
        Style::default().fg(Color::Red)
    } else if line.starts_with('@') {
        Style::default().fg(ACCENT)
    } else {
        Style::default()
    }
}

fn draw_conflicts(frame: &mut Frame, app: &App, area: Rect) {
    if app.conflicts.is_empty() {
        let block = Block::default().borders(Borders::ALL).title(" Conflitos ");
        frame.render_widget(Paragraph::new("Nenhum conflito pendente.").block(block), area);
        return;
    }

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(area);

    let items: Vec<ListItem> = app
        .conflicts
        .iter()
        .map(|c| ListItem::new(format!("{}: {}", c.module, c.path)))
        .collect();
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" Arquivos "))
        .highlight_style(highlight())
        .highlight_symbol("> ");
    let mut state = ListState::default().with_selected(Some(app.conflict_sel));
    frame.render_stateful_widget(list, cols[0], &mut state);

    let lines: Vec<Line> = app
        .diff_text
        .lines()
        .map(|l| Line::from(Span::styled(l, diff_line_style(l))))
        .collect();
    let diff = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Diff: atual → versão nova   [m] manter o meu  [n] usar o novo "),
        )
        .scroll((app.diff_scroll, 0));
    frame.render_widget(diff, cols[1]);
}

fn draw_report(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default().borders(Borders::ALL).title(" Último sync ");
    match &app.report {
        None => frame.render_widget(
            Paragraph::new("Nenhum sync executado ainda. Pressione s.").block(block),
            area,
        ),
        Some(report) => {
            let lines: Vec<Line> = report_lines(report).into_iter().map(Line::from).collect();
            frame.render_widget(
                Paragraph::new(lines)
                    .block(block)
                    .scroll((app.report_scroll, 0))
                    .wrap(Wrap { trim: false }),
                area,
            );
        }
    }
}

fn draw_help(frame: &mut Frame, area: Rect) {
    let section = |t: &'static str| Line::from(Span::styled(t, bold()));
    let left = vec![
        section("Navegação"),
        Line::from("  Tab / Shift-Tab, 1-5  trocar de tela"),
        Line::from("  ↑ ↓  ou  j k          mover a seleção"),
        Line::from(""),
        section("Módulos"),
        Line::from("  e / espaço / Enter    habilitar ou desabilitar"),
        Line::from("  o                     ir para as opções do módulo"),
        Line::from("  p                     fixar versão (0.10.*); vazio remove"),
        Line::from(""),
        section("Opções"),
        Line::from("  ← → ou h l            valor anterior / seguinte"),
        Line::from("  Shift + ← →           ±10 em números"),
        Line::from("  Enter                 lista com pré-visualização / digitar"),
        Line::from("  d                     voltar ao padrão"),
        Line::from("  a                     aplicar ao vivo (sync só do módulo)"),
    ];
    let right = vec![
        section("Conflitos"),
        Line::from("  m                     manter o meu arquivo"),
        Line::from("  n                     usar a versão nova (com backup)"),
        Line::from("  PgUp / PgDn  ou  u d  rolar o diff"),
        Line::from(""),
        section("Geral"),
        Line::from("  s                     sincronizar tudo agora"),
        Line::from("  w                     refazer o assistente"),
        Line::from("  r                     recarregar o estado"),
        Line::from("  q                     sair   (Ctrl-C força a saída)"),
    ];
    let block = Block::default().borders(Borders::ALL).title(" Ajuda ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(inner);
    frame.render_widget(Paragraph::new(left), cols[0]);
    frame.render_widget(Paragraph::new(right), cols[1]);
}

fn draw_log(frame: &mut Frame, app: &App, area: Rect) {
    let title = if app.running {
        " Log  ⟳ sync em andamento "
    } else {
        " Log "
    };
    let inner_height = area.height.saturating_sub(2) as usize;
    let log_rows = inner_height.saturating_sub(1);
    let start = app.log.len().saturating_sub(log_rows);

    let mut lines: Vec<Line> = app.log[start..].iter().map(|l| Line::from(l.as_str())).collect();
    lines.push(Line::from(Span::styled(
        app.status.as_str(),
        Style::default().fg(Color::Yellow),
    )));
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(title)),
        area,
    );
}

// ---------------------------------------------------------------- assistente

fn draw_wizard(frame: &mut Frame, app: &App, w: &Wizard, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(3),
        ])
        .split(area);

    // Cabeçalho com as etapas.
    let mut steps: Vec<Span> = Vec::new();
    for (i, step) in Step::ALL.iter().enumerate() {
        if i > 0 {
            steps.push(Span::styled(" › ", dim()));
        }
        let style = if *step == w.step {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else if i < w.step.index() {
            Style::default()
        } else {
            dim()
        };
        steps.push(Span::styled(format!("{} {}", i + 1, step.title()), style));
    }
    let title = if w.first_run {
        " hiraku · instalação "
    } else {
        " hiraku · refazer configuração "
    };
    frame.render_widget(
        Paragraph::new(Line::from(steps)).block(Block::default().borders(Borders::ALL).title(title)),
        chunks[0],
    );

    let body = chunks[1];
    match w.step {
        Step::Mode => draw_wizard_mode(frame, app, w, body),
        Step::Editor => {
            let items = EDITOR_CHOICES
                .iter()
                .map(|(label, ids)| {
                    let detail = if ids.is_empty() {
                        String::new()
                    } else {
                        format!("  ({})", ids.join(" + "))
                    };
                    ListItem::new(Line::from(vec![Span::raw(*label), Span::styled(detail, dim())]))
                })
                .collect();
            draw_choice_list(frame, body, "Qual editor você quer?", items, w.editor_sel);
        }
        Step::Modules => draw_wizard_modules(frame, w, body),
        Step::Options => draw_options_panel(
            frame,
            app,
            body,
            &w.option_specs(),
            &w.draft,
            w.option_sel,
            false,
        ),
        Step::Updates => draw_wizard_updates(frame, w, body),
        Step::Confirm => draw_wizard_confirm(frame, w, body),
    }

    let hint = match w.step {
        Step::Modules => "↑↓ move · espaço marca · a marca tudo · Enter/Tab segue · Esc volta · q sai",
        Step::Options => "Tab segue · Shift-Tab/Esc volta · q sai",
        Step::Confirm => "Enter instala agora · Esc volta · q sai sem gravar",
        _ => "↑↓ escolhe · Enter/Tab segue · Esc volta · q sai sem gravar",
    };
    frame.render_widget(
        Paragraph::new(Span::styled(hint, dim())).block(Block::default().borders(Borders::ALL)),
        chunks[2],
    );
}

fn draw_choice_list(frame: &mut Frame, area: Rect, title: &str, items: Vec<ListItem>, sel: usize) {
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(format!(" {title} ")))
        .highlight_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
        .highlight_symbol("● ");
    let mut state = ListState::default().with_selected(Some(sel));
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_wizard_mode(frame: &mut Frame, app: &App, w: &Wizard, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(6), Constraint::Min(3)])
        .split(area);
    let mut intro = vec![];
    if w.first_run {
        intro.push(Line::from(Span::styled(
            "Bem-vindo! Isto configura kitty, tmux, Vim/Neovim e o shell — e depois fica como hub.",
            bold(),
        )));
    }
    intro.push(Line::from(vec![
        Span::styled("Este computador: ", dim()),
        Span::raw(app.env_label.clone()),
    ]));
    intro.push(Line::from(Span::styled(
        "Nada é gravado até o último passo. Arquivos seus são copiados para backup antes de mudar.",
        dim(),
    )));
    frame.render_widget(
        Paragraph::new(intro)
            .wrap(Wrap { trim: true })
            .block(Block::default().borders(Borders::ALL)),
        rows[0],
    );

    let items = MODE_CHOICES
        .iter()
        .enumerate()
        .map(|(i, (label, detail))| {
            let mut spans = vec![Span::raw(*label), Span::styled(format!(" — {detail}"), dim())];
            if i == 2 && !w.can_root {
                spans.push(Span::styled("  (sudo não detectado)", Style::default().fg(Color::Yellow)));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();
    draw_choice_list(frame, rows[1], "Você tem acesso a sudo nesta máquina?", items, w.mode_sel);
}

fn draw_wizard_modules(frame: &mut Frame, w: &Wizard, area: Rect) {
    let items: Vec<ListItem> = w
        .choices
        .iter()
        .map(|m| {
            let check = if w.is_enabled(&m.id) { "[x]" } else { "[ ]" };
            let deps = if m.requires.is_empty() { "" } else { " +" };
            ListItem::new(Line::from(vec![
                Span::raw(format!("{check} {:<20}", m.id)),
                Span::styled(format!("{deps:<3}"), Style::default().fg(Color::Blue)),
                Span::styled(m.description.clone(), dim()),
            ]))
        })
        .collect();
    let count = w.draft.modules.len();
    let footer = match w.choices.get(w.module_sel) {
        Some(m) if !m.requires.is_empty() => {
            format!(" {} traz junto: {} ", m.id, m.requires.join(", "))
        }
        _ => " + = instala dependências junto ".to_string(),
    };
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" O que instalar? ({count} marcados) "))
                .title_bottom(Line::from(Span::styled(footer, Style::default().fg(Color::Blue)))),
        )
        .highlight_style(highlight())
        .highlight_symbol("> ");
    let mut state = ListState::default().with_selected(Some(w.module_sel));
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_wizard_updates(frame: &mut Frame, w: &Wizard, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(7), Constraint::Min(3)])
        .split(area);
    let items = UPDATE_CHOICES
        .iter()
        .map(|(label, _)| ListItem::new(*label))
        .collect();
    draw_choice_list(frame, rows[0], "Buscar atualizações sozinho?", items, w.update_sel);
    let text = vec![
        Line::from("O agendamento roda `hiraku sync --unattended` em segundo plano:"),
        Line::from("  • atualiza programas (kitty, nvim, node…) — exceto os com versão fixada"),
        Line::from("  • puxa configs novas do registry e faz merge com as suas edições"),
        Line::from("  • nunca pede senha e nunca sobrescreve um arquivo que você mudou:"),
        Line::from("    vira conflito pendente na tela Conflitos"),
    ];
    frame.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL)),
        rows[1],
    );
}

fn draw_wizard_confirm(frame: &mut Frame, w: &Wizard, area: Rect) {
    let mut lines = vec![Line::from("")];
    for (k, v) in w.summary() {
        lines.push(Line::from(vec![
            Span::styled(format!("  {k:<16}"), dim()),
            Span::raw(v),
        ]));
    }
    lines.push(Line::from(""));
    if w.draft.modules.is_empty() {
        lines.push(Line::from(Span::styled(
            "  Nenhum módulo marcado: volte (Esc) e escolha algo.",
            Style::default().fg(Color::Yellow),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "  Enter grava ~/.config/hiraku/settings.toml e instala. Depois disso esta tela vira o hub:",
            bold(),
        )));
        lines.push(Line::from("  rode `hiraku` de novo quando quiser mudar paleta, opacidade, módulos…"));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title(" Revisar ")),
        area,
    );
}

// ---------------------------------------------------------------- janelas

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    }
}

fn draw_editor(frame: &mut Frame, app: &App, editor: &Editor) {
    match editor {
        Editor::Pick { spec, values, sel } => {
            let has_preview = app.current_preview().is_some();
            let width = if has_preview { 84 } else { 40 };
            let height = (values.len() as u16 + 2).clamp(5, 20);
            let area = centered(frame.area(), width, height.max(if has_preview { 12 } else { 0 }));
            frame.render_widget(Clear, area);
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(ACCENT))
                .title(format!(" {}.{} ", spec.module, spec.key))
                .title_bottom(Line::from(Span::styled(" Enter escolhe · Esc cancela ", dim())));
            let inner = block.inner(area);
            frame.render_widget(block, area);
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints(if has_preview {
                    [Constraint::Length(24), Constraint::Min(0)]
                } else {
                    [Constraint::Min(0), Constraint::Length(0)]
                })
                .split(inner);
            let current = spec.value_in(match &app.wizard {
                Some(w) => &w.draft,
                None => &app.settings,
            })
            .0;
            let items: Vec<ListItem> = values
                .iter()
                .map(|v| {
                    let mark = if *v == current { " ✓" } else { "" };
                    ListItem::new(format!("{v}{mark}"))
                })
                .collect();
            let list = List::new(items)
                .highlight_style(highlight())
                .highlight_symbol("> ");
            let mut state = ListState::default().with_selected(Some(*sel));
            frame.render_stateful_widget(list, cols[0], &mut state);
            if let Some(preview) = app.current_preview() {
                let pad = Rect {
                    x: cols[1].x + 1,
                    width: cols[1].width.saturating_sub(1),
                    ..cols[1]
                };
                draw_preview(frame, pad, preview);
            }
        }
        Editor::Input {
            target,
            buffer,
            error,
        } => {
            let (title, hint) = match target {
                EditTarget::Option(spec) => (format!(" {}.{} ", spec.module, spec.key), spec.hint()),
                EditTarget::Pin(module) => (
                    format!(" fixar versão de {module} "),
                    "ex.: 0.10.* ou 3.4 — vazio remove o pin".to_string(),
                ),
            };
            let area = centered(frame.area(), 56, 7);
            frame.render_widget(Clear, area);
            let lines = vec![
                Line::from(vec![Span::raw(buffer.clone()), Span::styled("▏", Style::default().fg(ACCENT))]),
                Line::from(Span::styled(hint, dim())),
                Line::from(Span::styled(
                    error.clone().unwrap_or_default(),
                    Style::default().fg(Color::Red),
                )),
            ];
            frame.render_widget(
                Paragraph::new(lines).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(ACCENT))
                        .title(title)
                        .title_bottom(Line::from(Span::styled(" Enter grava · Esc cancela ", dim()))),
                ),
                area,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{ConflictRow, ModuleRow, Snapshot};
    use crate::catalog::ModuleInfo;
    use hiraku_core::sync::{ConfigResult, InstallStatus, ModuleReport, SyncReport};
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::Terminal;

    fn render(app: &App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut out = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                out.push_str(buffer[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    fn row(id: &str, enabled: bool, installed: Option<&str>, pin: Option<&str>) -> ModuleRow {
        ModuleRow {
            id: id.to_string(),
            description: format!("desc {id}"),
            enabled,
            installed: installed.map(str::to_string),
            pin: pin.map(str::to_string),
            supported: true,
        }
    }

    fn kitty() -> ModuleInfo {
        ModuleInfo {
            id: "kitty".into(),
            name: "kitty".into(),
            description: "terminal".into(),
            supported: true,
            requires: vec!["font".into()],
            options: vec![
                OptSpec {
                    module: "kitty".into(),
                    key: "palette".into(),
                    kind: OptKind::Choice(vec!["navy".into(), "nord".into()]),
                    default: "navy".into(),
                    description: "Paleta de cores".into(),
                    ..Default::default()
                },
                OptSpec {
                    module: "kitty".into(),
                    key: "opacity".into(),
                    kind: OptKind::Number {
                        min: 30,
                        max: 100,
                        divisor: 100,
                    },
                    default: "80".into(),
                    description: "Opacidade".into(),
                    ..Default::default()
                },
            ],
        }
    }

    fn hub_app() -> App {
        let mut app = App::new();
        let mut settings = Settings::default();
        settings.enable("kitty");
        settings.set_option("kitty", "opacity", "90");
        app.apply_snapshot(Snapshot {
            modules: vec![row("kitty", true, Some("0.36.4"), None)],
            catalog: vec![kitty()],
            settings,
            ..Snapshot::default()
        });
        app
    }

    fn key(app: &mut App, code: KeyCode) {
        app.on_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    #[test]
    fn modules_screen_shows_state() {
        let mut app = App::new();
        app.apply_snapshot(Snapshot {
            modules: vec![
                row("kitty", true, Some("0.36.4"), Some("0.36.*")),
                row("tmux", false, None, None),
            ],
            ..Snapshot::default()
        });
        let text = render(&app, 100, 30);
        assert!(text.contains("hiraku"));
        assert!(text.contains("[x] kitty"));
        assert!(text.contains("0.36.4"));
        assert!(text.contains("pin 0.36.*"));
        assert!(text.contains("[ ] tmux"));
        assert!(text.contains("desc tmux"));
        assert!(text.contains("Conflitos (0)"));
        assert!(text.contains("2 Opções"));
        assert!(text.contains("pronto"));
    }

    #[test]
    fn empty_registry_is_explained() {
        let app = App::new();
        assert!(render(&app, 80, 24).contains("Nenhum módulo no registry"));
    }

    #[test]
    fn options_screen_shows_values_details_and_preview() {
        let mut app = hub_app();
        app.screen = Screen::Options;
        app.set_preview(
            "kitty".into(),
            "palette".into(),
            "navy".into(),
            vec![("background".into(), (0, 0, 0x40)), ("color2".into(), (0, 0xff, 0))],
        );
        let text = render(&app, 120, 34);
        assert!(text.contains("‹ navy ›"));
        assert!(text.contains("‹ 90% ›"), "valor custom aparece");
        assert!(text.contains("•"), "marcador de valor alterado");
        assert!(text.contains("Paleta de cores"));
        assert!(text.contains("(padrão)"));
        assert!(text.contains("pré-visualização: navy"));
        assert!(text.contains("hiraku sync"));
    }

    #[test]
    fn picker_popup_lists_values() {
        let mut app = hub_app();
        app.screen = Screen::Options;
        key(&mut app, KeyCode::Enter);
        let text = render(&app, 120, 34);
        assert!(text.contains("kitty.palette"));
        assert!(text.contains("navy ✓"));
        assert!(text.contains("Enter escolhe"));
    }

    #[test]
    fn input_popup_shows_errors() {
        let mut app = hub_app();
        app.screen = Screen::Options;
        key(&mut app, KeyCode::Down);
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Char('0'));
        key(&mut app, KeyCode::Enter);
        let text = render(&app, 120, 34);
        assert!(text.contains("900"));
        assert!(text.contains("fora do intervalo"));
    }

    #[test]
    fn wizard_screens_render() {
        let mut app = hub_app();
        app.env_label = "linux · fedora · sem sudo".into();
        app.start_wizard(true);
        let text = render(&app, 120, 34);
        assert!(text.contains("instalação"));
        assert!(text.contains("Bem-vindo"));
        assert!(text.contains("fedora"));
        assert!(text.contains("(sudo não detectado)"));

        key(&mut app, KeyCode::Enter);
        assert!(render(&app, 120, 34).contains("Qual editor"));
        key(&mut app, KeyCode::Enter);
        let text = render(&app, 120, 34);
        assert!(text.contains("[x] kitty"));
        assert!(text.contains(" + "));
        key(&mut app, KeyCode::Enter);
        assert!(render(&app, 120, 34).contains("‹ navy ›"));
        key(&mut app, KeyCode::Tab);
        assert!(render(&app, 120, 34).contains("Uma vez por dia"));
        key(&mut app, KeyCode::Enter);
        let text = render(&app, 120, 34);
        assert!(text.contains("Revisar"));
        assert!(text.contains("Enter grava"));
    }

    #[test]
    fn conflicts_screen_shows_files_and_diff() {
        let mut app = App::new();
        app.apply_snapshot(Snapshot {
            conflicts: vec![ConflictRow {
                path: "/home/ana/.config/kitty/kitty.conf".to_string(),
                module: "kitty".to_string(),
            }],
            ..Snapshot::default()
        });
        app.screen = Screen::Conflicts;
        app.set_diff(
            "/home/ana/.config/kitty/kitty.conf".to_string(),
            Ok("@@ -1 +1 @@\n-minha\n+deles\n".to_string()),
        );
        let text = render(&app, 120, 30);
        assert!(text.contains("Conflitos (1)"));
        assert!(text.contains("kitty:"));
        assert!(text.contains("+deles"));
        assert!(text.contains("-minha"));
        assert!(text.contains("manter o meu"));
    }

    #[test]
    fn conflicts_screen_without_conflicts() {
        let mut app = App::new();
        app.screen = Screen::Conflicts;
        assert!(render(&app, 80, 24).contains("Nenhum conflito pendente"));
    }

    #[test]
    fn report_screen_lists_modules() {
        let mut app = App::new();
        app.apply_snapshot(Snapshot {
            report: Some(SyncReport {
                started_at: 1,
                finished_at: 2,
                registry_version: 7,
                registry_source: "remote".to_string(),
                notes: vec!["uma nota".to_string()],
                modules: vec![ModuleReport {
                    id: "nvim".to_string(),
                    status: InstallStatus::Updated,
                    detail: Some("0.10.2 -> 0.11.0".to_string()),
                    major: true,
                    error: None,
                    configs: vec![ConfigResult {
                        dest: "/h/.config/nvim/init.lua".to_string(),
                        outcome: "Merged".to_string(),
                    }],
                    hooks: vec![],
                }],
            }),
            ..Snapshot::default()
        });
        app.screen = Screen::Report;
        let text = render(&app, 120, 30);
        assert!(text.contains("registry v7"));
        assert!(text.contains("uma nota"));
        assert!(text.contains("nvim Updated [MAJOR] 0.10.2 -> 0.11.0"));
        assert!(text.contains("Merged: /h/.config/nvim/init.lua"));
    }

    #[test]
    fn report_screen_without_report() {
        let mut app = App::new();
        app.screen = Screen::Report;
        assert!(render(&app, 80, 24).contains("Nenhum sync executado"));
    }

    #[test]
    fn help_screen_lists_keys() {
        let mut app = App::new();
        app.screen = Screen::Help;
        let text = render(&app, 120, 34);
        assert!(text.contains("manter o meu arquivo"));
        assert!(text.contains("refazer o assistente"));
        assert!(text.contains("Ctrl-C"));
    }

    #[test]
    fn log_shows_the_tail_and_the_running_marker() {
        let mut app = App::new();
        app.running = true;
        for i in 0..50 {
            app.push_log(format!("evento {i}"));
        }
        let text = render(&app, 100, 30);
        assert!(text.contains("sync em andamento"));
        assert!(text.contains("evento 49"));
        assert!(!text.contains("evento 0 "), "só o fim do log cabe na tela");
    }

    #[test]
    fn tiny_terminals_do_not_panic() {
        let mut app = hub_app();
        app.apply_snapshot(Snapshot {
            modules: vec![row("a", true, None, None)],
            conflicts: vec![ConflictRow {
                path: "/x".to_string(),
                module: "m".to_string(),
            }],
            catalog: vec![kitty()],
            ..Snapshot::default()
        });
        app.set_preview("kitty".into(), "palette".into(), "navy".into(), vec![("background".into(), (1, 1, 1))]);
        let sizes = [(1, 1), (10, 3), (20, 5), (40, 10), (80, 24)];
        for screen in Screen::ALL {
            app.screen = screen;
            for (w, h) in sizes {
                render(&app, w, h);
            }
        }
        app.screen = Screen::Options;
        key(&mut app, KeyCode::Enter);
        for (w, h) in sizes {
            render(&app, w, h);
        }
        app.editor = None;
        app.start_wizard(true);
        for _ in 0..6 {
            for (w, h) in sizes {
                render(&app, w, h);
            }
            key(&mut app, KeyCode::Tab);
        }
    }
}
