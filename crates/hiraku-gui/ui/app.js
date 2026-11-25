"use strict";
/* Hiraku Setup Manager — interface. Fala com o backend Rust (Tauri) por `call`.
   Aberto num navegador comum (sem Tauri), usa dados de exemplo (MOCK) para
   desenvolver a interface. */

const T = window.__TAURI__;
const $ = (s, el = document) => el.querySelector(s);
const $$ = (s, el = document) => [...el.querySelectorAll(s)];
const esc = s => String(s ?? "").replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));

// ------------------------------------------------------------------ backend
async function call(cmd, args = {}) {
  if (T && T.core) return T.core.invoke(cmd, args);
  return MOCK.call(cmd, args);
}
function on(event, fn) {
  if (T && T.event) return T.event.listen(event, e => fn(e.payload));
  MOCK.listeners[event] = fn;
}

// ------------------------------------------------------------------ estado
let INFO = null;          // ambiente, versão, primeira execução
let SNAP = null;          // módulos, conflitos, relatório, catálogo, settings
const PREV = {};          // "kitty.palette" -> {valor: {background: "#..", ...}}
let VIEW = "home";
let SYNC = null;          // {only, lines:[], progress, startedAt}
let CONFLICT_SEL = 0;
let MOD_FILTER = "all";
const UI = Object.assign({ theme: "auto", accent: "#c2577f", font: "serif", live: true, every: 86400, autoApp: true },
  JSON.parse(localStorage.getItem("hsm.ui") || "{}"));
const saveUI = () => localStorage.setItem("hsm.ui", JSON.stringify(UI));

const THEMES = [["auto", "Automático", "linear-gradient(135deg,#f3f1ee 50%,#15141a 50%)", "#7a6db0", "#a498cf"], ["light", "Claro", "#f3f1ee", "#fff", "#7a6db0"], ["dark", "Escuro", "#15141a", "#1d1c24", "#a498cf"], ["sepia", "Sépia", "#f1e8d6", "#faf3e3", "#9a5b2e"], ["sakura", "Sakura", "#f8f0f2", "#fffafb", "#c2577f"], ["midnight", "Meia-noite", "#000", "#0b0b0f", "#b3a7ff"], ["ocean", "Oceano", "#0e1a24", "#13232f", "#5cb8d6"], ["forest", "Floresta", "#111a15", "#17231c", "#7cc08f"], ["nord", "Nord", "#2e3440", "#3b4252", "#88c0d0"], ["dracula", "Drácula", "#282a36", "#343746", "#bd93f9"], ["mocha", "Mocha", "#1e1e2e", "#272739", "#f5c2e7"], ["sunset", "Pôr do sol", "#1c1419", "#271b22", "#ff8a5b"], ["solar", "Solar", "#fdf6e3", "#fffaf0", "#268bd2"], ["lavender", "Lavanda", "#f1eefb", "#fbfaff", "#7c5cd6"], ["mint", "Menta", "#eef6f1", "#fafffc", "#2f9e6e"]];
const THEME_ACC = Object.fromEntries(THEMES.map(t => [t[0], t[4]]));
const ACCS = ["", "#7a6db0", "#c2577f", "#d9822b", "#3f9d77", "#3a86c8", "#c0504d"];
const FONTS = { serif: 'Georgia,"Times New Roman",serif', book: '"Iowan Old Style","Palatino Linotype",Palatino,Georgia,serif', sans: '"Segoe UI",system-ui,-apple-system,sans-serif', mono: 'ui-monospace,"Cascadia Code",Consolas,monospace' };

// Mesmas escolhas do assistente da TUI (hiraku-hub/presets.rs).
const RECOMMENDED = ["font-jetbrains-mono", "kitty", "tmux", "vim", "nvim", "node", "jdk", "ripgrep", "shell", "starship", "eza", "bat", "zoxide", "fzf", "fastfetch"];
const MODES = [
  [null, "Automático", "Usa os pacotes da distro se houver sudo; senão, tudo em ~/.local.", "✦"],
  ["user", "Só no meu usuário", "Sem sudo: tudo em ~/.local. Ideal para PC de laboratório.", "⌂"],
  ["system", "Sistema, com sudo", "dnf/apt/pacman/brew quando possível. A senha é pedida uma vez.", "⚿"],
];
const EDITORS = [["Vim e Neovim", ["vim", "nvim"], "Os dois, com os mesmos atalhos e temas.", "⇄"], ["Só Neovim", ["nvim"], "LSP nativo, Treesitter e lazy.nvim.", "N"], ["Só Vim", ["vim"], "Clássico, com coc.nvim e plugins.", "V"], ["Nenhum editor", [], "Só terminal, shell e utilitários.", "∅"]];
const UPDATES = [[21600, "A cada 6 horas", "Fica sempre em dia sem você lembrar."], [86400, "Uma vez por dia", "Recomendado: atualiza em segundo plano."], [null, "Só quando eu mandar", "Nada roda sozinho; use Sincronizar."]];
const GLYPH = { kitty: "K", tmux: "T", vim: "V", nvim: "N", node: "⬢", jdk: "J", shell: "$", starship: "✦", eza: "≡", bat: "B", zoxide: "Z", fzf: "F", fastfetch: "ℹ", ripgrep: "R", "font-jetbrains-mono": "Aa", "icaro-themes": "❖", "dev-prereqs": "⚙" };

const applogo = () => '<svg class="applogo" viewBox="0 0 100 100" aria-hidden="true"><path fill="url(#lgA)" fill-rule="evenodd" d="M50 3C74 3 93 24 93 50S74 97 50 97 7 76 7 50 26 3 50 3ZM50 10C64 10 73 28 73 50S64 90 50 90 27 72 27 50 36 10 50 10Z"/><path fill="none" style="stroke:color-mix(in srgb,var(--acc) 70%,#fff)" stroke-width="5.5" stroke-linecap="round" stroke-linejoin="round" d="M40 38 50 48 40 58"/><path style="stroke:color-mix(in srgb,var(--acc) 70%,#fff)" stroke-width="5.5" stroke-linecap="round" d="M53 59H62"/></svg>';
// Logos do projeto IcaroPintoLira (public/assets): uma por programa configurado.
const LOGO = { kitty: "terminal", shell: "terminal", tmux: "tmux", nvim: "neovim", vim: "vim", "vim-classico": "vim" };
const tile = (m, cls = "") => LOGO[m.id]
  ? `<span class="pj-tile logo ${cls}"><img src="logos/${LOGO[m.id]}.svg" alt="" draggable="false"></span>`
  : `<span class="pj-tile ${cls}">${esc(GLYPH[m.id] || m.name[0].toUpperCase())}</span>`;

// ------------------------------------------------------------------ util
function toast(msg, kind = "", ms = 4200) {
  const t = document.createElement("div");
  t.className = "toast " + kind;
  t.textContent = msg;
  $("#toasts").appendChild(t);
  setTimeout(() => t.remove(), ms);
}
const fail = e => toast(String(e), "err", 7000);
function ago(secs) {
  if (!secs) return "nunca";
  const d = Math.max(0, Date.now() / 1000 - secs);
  if (d < 60) return "agora há pouco";
  if (d < 3600) return `há ${Math.round(d / 60)} min`;
  if (d < 86400) return `há ${Math.round(d / 3600)} h`;
  return `há ${Math.round(d / 86400)} dias`;
}
const catalog = () => (SNAP ? SNAP.catalog : []);
const modInfo = id => catalog().find(m => m.id === id);
const row = id => (SNAP ? SNAP.modules.find(m => m.id === id) : null);
const enabled = id => !!SNAP && SNAP.settings.modules.includes(id);
function optValue(spec, settings) {
  const v = settings.options?.[spec.module]?.[spec.key];
  return v === undefined ? spec.default : v;
}
function sortedSpecs(mods) {
  const rank = s => ({ choice: 0, number: 1, bool: 2, text: 3, color: 3, lines: 4, keymaps: 4 })[s.kind.type];
  // Módulos com seções (vim, nvim, tmux) seguem a ordem do manifest.
  return mods.flatMap(m => m.options.some(o => o.section) ? [...m.options]
    : [...m.options].sort((a, b) => rank(a) - rank(b) || a.key.localeCompare(b.key)));
}
function withDeps(ids) {
  const out = new Set(), stack = [...ids];
  while (stack.length) {
    const id = stack.pop();
    if (out.has(id)) continue;
    out.add(id);
    (modInfo(id)?.requires || []).forEach(r => stack.push(r));
  }
  return catalog().filter(m => out.has(m.id)).map(m => m.id);
}
const prettyKey = k => ({ palette: "Paleta de cores", opacity: "Opacidade", blur: "Desfoque", visualizer: "Ao abrir o terminal", fastfetch_color: "Cor do fastfetch", neofetch_color: "Cor do neofetch" })[k] || (k.charAt(0).toUpperCase() + k.slice(1)).replace(/_/g, " ");
const STATUS = { installed: ["✓", "ok", "instalado"], updated: ["↑", "ok", "atualizado"], unchanged: ["✓", "ok", "em dia"], config_only: ["✓", "ok", "configs aplicadas"], skipped: ["↷", "warn", "pulado"], unsupported: ["–", "", "não suportado"], failed: ["✗", "err", "falhou"] };

// ------------------------------------------------------------------ aparência do app
function applyUI() {
  const r = document.documentElement;
  if (UI.theme === "auto") r.removeAttribute("data-theme"); else r.dataset.theme = UI.theme;
  r.style.setProperty("--acc", UI.accent || THEME_ACC[UI.theme] || "#c2577f");
  r.style.setProperty("--serif", FONTS[UI.font] || FONTS.serif);
}

// ------------------------------------------------------------------ carga
async function reload() {
  try {
    [INFO, SNAP] = await Promise.all([call("info"), call("snapshot")]);
  } catch (e) { fail(e); return; }
  $("#envbadge").textContent = `${INFO.env_label}`;
  $("#appver").textContent = "v" + INFO.version;
  const n = SNAP.conflicts.length;
  $("#nconf").hidden = !n; $("#nconf").textContent = n;
  render();
}
async function previews(module, key) {
  const k = module + "." + key;
  if (!PREV[k]) {
    try { PREV[k] = await call("previews", { module, key }); } catch { PREV[k] = {}; }
  }
  return PREV[k];
}

// ------------------------------------------------------------------ navegação
function go(v) { VIEW = v; render(); $("#view").scrollTop = 0; }
function render() {
  if (!SNAP) return;
  $$("nav button[data-v]").forEach(b => b.classList.toggle("on", b.dataset.v === VIEW));
  const fn = { home: viewHome, modules: viewModules, custom: viewCustom, conflicts: viewConflicts, updates: viewUpdates }[VIEW];
  $("#view").innerHTML = fn();
  bindView();
  if (VIEW === "custom") hydrateCustom($("#view"), SNAP.settings, liveChange);
  if (VIEW === "conflicts") loadDiff();
}

function ring(frac, label) {
  const C = 2 * Math.PI * 28, f = Math.max(0, Math.min(1, frac));
  return `<div class="hx-ring"><svg viewBox="0 0 64 64"><circle class="trk" cx="32" cy="32" r="28"/>${f > 0 ? `<circle class="arc" cx="32" cy="32" r="28" stroke-dasharray="${(C * f).toFixed(1)} ${C}"/>` : ""}</svg><b>${esc(label)}</b></div>`;
}

// ------------------------------------------------------------------ Início
function viewHome() {
  const s = SNAP.settings, en = s.modules.length;
  const inst = s.modules.filter(id => row(id)?.installed).length;
  const rep = SNAP.report;
  const kitty = modInfo("kitty"), pal = kitty && optValue(kitty.options.find(o => o.key === "palette") || { default: "" }, s);
  const modeTxt = s.mode === "user" ? "só usuário" : s.mode === "system" ? "sistema (sudo)" : "automático";
  const hero = `<section class="hx-hero"><div class="hx-logo">${applogo()}</div><div class="hx-hmain">
    <h2>Seu setup</h2>
    <p class="mut">${en} módulo(s) mantidos em dia · modo ${modeTxt} · registry v${INFO.registry_version} (${esc(INFO.registry_source)})</p>
    <div class="hx-cta"><button class="hx-btn" data-act="sync" ${SYNC ? "disabled" : ""}>${SYNC ? "Sincronizando…" : "Sincronizar agora"}</button>
    <button class="hx-btn ghost" data-act="health">Verificar integridade</button>
    <button class="hx-btn ghost" data-v="custom">Personalizar</button><button class="hx-btn ghost" data-act="wizard">Refazer instalação…</button></div></div></section>`;
  const orbs = `<div class="hx-orbs">
    <button class="hx-orb" data-v="modules">${ring(en ? inst / en : 0, `${inst}/${en}`)}<small>instalados</small></button>
    <button class="hx-orb" data-v="conflicts">${ring(SNAP.conflicts.length ? 1 : 0, SNAP.conflicts.length)}<small>conflitos pendentes</small></button>
    <button class="hx-orb" data-v="updates">${ring(SNAP.scheduled ? 1 : 0, SNAP.scheduled ? "on" : "off")}<small>atualização automática</small></button>
    <button class="hx-orb" data-v="updates">${ring(rep && !hasFailures(rep) ? 1 : rep ? .5 : 0, rep ? (hasFailures(rep) ? "!" : "✓") : "–")}<small>último sync ${rep ? ago(rep.finished_at) : "nunca"}</small></button></div>`;
  const quick = [];
  if (kitty) {
    const pv = PREV["kitty.palette"]?.[pal];
    const dots = pv ? `<span class="swdots">${[1, 2, 3, 4, 5, 6].map(n => `<i style="background:${pv["color" + n]}"></i>`).join("")}</span>` : "";
    quick.push(`<button class="hx" data-goto="kitty"><span class="hxi">K</span><b>Paleta do terminal</b><small>${esc(pal)} ${dots}</small></button>`);
    const op = optValue(kitty.options.find(o => o.key === "opacity") || { default: "100" }, s);
    quick.push(`<button class="hx" data-goto="kitty"><span class="hxi">▒</span><b>Transparência</b><small>${op === "100" ? "sólido" : `${op}% de opacidade`}</small></button>`);
  }
  const eds = ["vim", "nvim"].filter(enabled).map(e => e === "nvim" ? "Neovim" : "Vim");
  quick.push(`<button class="hx" data-v="modules"><span class="hxi">✎</span><b>Editores</b><small>${eds.length ? eds.join(" + ") : "nenhum habilitado"}</small></button>`);
  if (modInfo("shell")) quick.push(`<button class="hx" data-goto="shell"><span class="hxi">$</span><b>Shell</b><small>ao abrir: ${esc(optValue(modInfo("shell").options.find(o => o.key === "visualizer") || { default: "none" }, s))}</small></button>`);
  quick.push(`<button class="hx" data-v="updates"><span class="hxi">↻</span><b>Atualizações</b><small>${SNAP.scheduled ? "automáticas" : "manuais"}</small></button>`);

  const repList = rep ? rep.modules.map(m => {
    const [ic, cls, txt] = STATUS[m.status] || ["•", "", m.status];
    return `<div class="hx-li click" data-v="updates"><span class="hx-dot ${cls}">${ic}</span><div><b>${esc(m.id)}</b><small>${esc(m.error || m.detail || txt)}</small></div></div>`;
  }).join("") : `<small class="mut">Nenhum sync ainda.</small>`;
  const rail = `<aside class="hx-rail">
    <div class="hx-panel"><h6>Último sync <span class="n">${rep ? ago(rep.finished_at) : ""}</span></h6>${repList}</div>
    <div class="hx-panel"><h6>Este computador</h6><dl class="kv"><dt>sistema</dt><dd>${esc(INFO.env_label)}</dd><dt>modo</dt><dd>${modeTxt}</dd><dt>configs</dt><dd title="${esc(INFO.settings_file)}">${esc(INFO.settings_file)}</dd></dl>
    <div class="hx-cta" style="margin-top:10px"><button class="hx-btn ghost sm" data-act="opendir">Abrir pasta</button></div></div></aside>`;
  return `<div class="hx-wrap"><div class="hx-main">${hero}${orbs}<div class="hx-sec">Acesso rápido</div><div class="hx-grid">${quick.join("")}</div></div>${rail}</div>`;
}
const hasFailures = r => r.modules.some(m => m.status === "failed");

// ------------------------------------------------------------------ Módulos
function viewModules() {
  const mods = catalog().filter(m => m.supported || enabled(m.id));
  const filt = { all: () => true, on: m => enabled(m.id), off: m => !enabled(m.id), inst: m => !!row(m.id)?.installed }[MOD_FILTER];
  const rep = SNAP.report ? Object.fromEntries(SNAP.report.modules.map(m => [m.id, m])) : {};
  const cards = mods.filter(filt).map(m => {
    const r = row(m.id) || {}, on = enabled(m.id), last = rep[m.id];
    const st = last ? STATUS[last.status] : null;
    const opts = m.options.length ? `<button class="hx-btn ghost sm" data-goto="${m.id}">Opções</button>` : "";
    return `<article class="pj-card ${on ? "on" : ""} ${m.supported ? "" : "off"}">
      <label class="sw pj-sw" title="${on ? "Desabilitar" : "Habilitar"} no sync"><input type="checkbox" data-toggle="${m.id}" ${on ? "checked" : ""} ${m.supported ? "" : "disabled"}><i></i></label>
      <div class="pj-top">${tile(m)}<div class="pj-main"><b>${esc(m.name)}</b><small>${m.requires.length ? "usa " + esc(m.requires.join(", ")) : esc(m.id)}</small></div></div>
      <div class="pj-desc">${esc(m.description)}</div>
      <div class="pj-path">${r.installed ? `<b>${esc(r.installed)}</b>` : "não instalado"}${r.pin ? ` · <span class="tag acc">fixado ${esc(r.pin)}</span>` : ""}${st ? ` · <span title="${esc(last.error || last.detail || "")}">${st[0]} ${st[2]}</span>` : ""}</div>
      <div class="pj-act">${opts}<button class="hx-btn ghost sm" data-pin="${m.id}">${r.pin ? "Mudar versão fixada" : "Fixar versão"}</button>${on && r.installed ? `<button class="hx-btn ghost sm" data-apply="${m.id}">Aplicar agora</button>` : ""}${r.installed ? `<button class="hx-btn ghost sm danger" data-uninstall="${m.id}">Desinstalar</button>` : ""}</div></article>`;
  }).join("");
  const chip = (k, t) => `<button class="chip ${MOD_FILTER === k ? "on" : ""}" data-filter="${k}">${t}</button>`;
  return `<div class="hx-wrap one"><div class="hx-main">
    <section class="hx-hero sm"><div class="hx-logo">${applogo()}</div><div class="hx-hmain"><h2>Módulos</h2><p class="mut">Ligue o que o hiraku deve instalar e manter em dia. Desligar só para de manter em dia; para tirar de vez, use Desinstalar.</p></div></section>
    <div class="hx-sec">Seus módulos <span class="n">${SNAP.settings.modules.length}/${mods.length}</span></div>
    <div class="bar">${chip("all", "Todos")}${chip("on", "Habilitados")}${chip("off", "Desligados")}${chip("inst", "Instalados")}</div>
    <div class="pj-grid">${cards || '<div class="hx-panel mut">Nada aqui com esse filtro.</div>'}</div></div></div>`;
}

// ------------------------------------------------------------------ Personalizar (também usado no assistente)
const MODE_LABELS = [["n", "Normal"], ["i", "Inserção"], ["v", "Visual"], ["nv", "Normal + Visual"], ["x", "Só visual"], ["t", "Terminal"], ["c", "Linha de comando"], ["o", "Operador"]];
const KEYMAP_PRESETS = [
  ["jk sai do modo inserção", { mode: "i", lhs: "jk", rhs: "<Esc>", desc: "Sair do modo inserção" }],
  ["<leader>y copia para o sistema", { mode: "nv", lhs: "<leader>y", rhs: "\"+y", desc: "Copiar para a área de transferência" }],
  ["<leader>p cola do sistema", { mode: "nv", lhs: "<leader>p", rhs: "\"+p", desc: "Colar da área de transferência" }],
  ["Ctrl+A seleciona tudo", { mode: "n", lhs: "<C-a>", rhs: "ggVG", desc: "Selecionar tudo" }],
  ["Y copia até o fim da linha", { mode: "n", lhs: "Y", rhs: "y$", desc: "Copiar até o fim da linha" }],
  ["Alt+Z liga/desliga quebra de linha", { mode: "n", lhs: "<A-z>", rhs: "<Cmd>set wrap!<CR>", desc: "Quebra de linha" }],
  ["<leader>a salva todos os arquivos", { mode: "n", lhs: "<leader>a", rhs: "<Cmd>wall<CR>", desc: "Salvar tudo" }],
  ["Desativar Q (modo Ex)", { mode: "n", lhs: "Q", rhs: "<Nop>", desc: "Desativado" }],
];
const TMUX_PRESETS = [
  ["Alt+Z maximiza o painel", "bind -n M-z resize-pane -Z"],
  ["Alt+1…5 vai para a janela", "bind -n M-1 select-window -t 1\nbind -n M-2 select-window -t 2\nbind -n M-3 select-window -t 3\nbind -n M-4 select-window -t 4\nbind -n M-5 select-window -t 5"],
  ["Prefixo + S sincroniza painéis", "bind S setw synchronize-panes"],
  ["Prefixo + e edita o tmux.conf", "bind e new-window -n tmux.conf \"$EDITOR ~/.tmux.conf\""],
  ["Desativar Ctrl+Z (suspender)", "unbind -n C-z"],
];

const specLabel = s => s.label || prettyKey(s.key);
const valueLabel = (s, v) => (s.labels && s.labels[v]) || (v === "" ? "(vazio)" : v);
const sectionsOf = specs => [...new Set(specs.map(s => s.section || ""))];

function customHTML(mods, settings, opts = {}) {
  const sections = mods.filter(m => m.options.length).map(m => {
    let specs = sortedSpecs([m]);
    // No assistente, dos editores só a aparência (o resto fica para o hub).
    if (opts.wizard) specs = specs.filter(s => !s.section || s.section === "Aparência");
    if (!specs.length) return "";
    const groups = sectionsOf(specs).map(sec => {
      const rows = specs.filter(s => (s.section || "") === sec).map(s => optionRow(s, settings)).join("");
      return `${sec ? `<div class="st-sec">${esc(sec)}</div>` : ""}${rows}`;
    }).join("");
    const hasTerm = m.options.some(o => o.key === "palette") && m.options.some(o => o.key === "opacity");
    const off = !opts.wizard && !enabled(m.id) ? `<span class="tag">desabilitado</span>` : "";
    const live = !opts.wizard && ["vim", "nvim", "tmux"].includes(m.id)
      ? `<span class="tag acc" title="O ${esc(m.name)} aberto percebe a mudança e reaplica sozinho">aplica com ele aberto</span>` : "";
    return `<section class="cz-sec glass" id="cz-${m.id}"><header>${tile(m)}<h3>${esc(m.name)}</h3>${live}${off}</header>
      ${hasTerm ? `<div class="termwrap"><div class="term" data-term="${m.id}"></div></div>` : ""}${groups}</section>`;
  }).filter(Boolean);
  if (!sections.length) return `<div class="hx-panel mut">Nenhum módulo escolhido tem opções.</div>`;
  if (opts.wizard) return sections.join("");
  const rail = mods.filter(m => m.options.length).map(m => `<button data-jump="${m.id}">${esc(m.name)}<span class="mut">${m.options.length}</span></button>`).join("");
  return `<div class="cz"><div class="cz-rail glass" role="navigation" aria-label="Módulos">${rail}</div><div>${sections.join("")}</div></div>`;
}

function optionRow(spec, settings) {
  const v = optValue(spec, settings), custom = v !== spec.default;
  const id = `${spec.module}.${spec.key}`;
  const reset = custom ? `<button class="reset" data-reset="${id}">voltar ao padrão</button>` : "";
  const lbl = `<div class="st-lbl"><b>${esc(specLabel(spec))}${reset}</b>${spec.description ? `<small>${esc(spec.description)}</small>` : ""}</div>`;
  switch (spec.kind.type) {
    case "choice": {
      const vals = spec.kind.data;
      if (spec.has_preview || spec.key === "palette") {
        const search = vals.length > 12 ? `<input class="txt th-search" placeholder="Filtrar ${vals.length} opções…" data-filter-grid="${id}">` : "";
        const code = spec.key !== "palette";
        return `<div class="st-row col">${lbl}${search}<div class="th-grid ${code ? "code" : ""}" data-pal="${id}">${vals.map(x => `<button class="th-card ${x === v ? "on" : ""}" data-set="${id}" data-val="${esc(x)}" data-name="${esc((valueLabel(spec, x) + " " + x).toLowerCase())}"><div class="th-prev" data-pv="${esc(x)}"></div><span>${esc(valueLabel(spec, x))}</span></button>`).join("")}</div></div>`;
      }
      // Valores com texto longo (details) viram uma linha do tempo, do mais novo ao mais antigo.
      if (spec.details && Object.keys(spec.details).length) {
        return `<div class="st-row col">${lbl}<ol class="tl">${vals.map((x, i) => {
          const [when, ...rest] = valueLabel(spec, x).split(" · ");
          const title = rest.join(" · ") || when;
          return `<li class="${x === v ? "on" : ""}"><button data-set="${id}" data-val="${esc(x)}"><span class="tl-dot"></span><span class="tl-when">${esc(rest.length ? when : "")}${i === 0 ? ' <em>mais nova</em>' : ""}${x === v ? ' <em class="cur">escolhida</em>' : ""}</span><b>${esc(title)}</b><small>${esc(spec.details[x] || "")}</small><code>${esc(x.slice(0, 7))}</code></button></li>`;
        }).join("")}</ol></div>`;
      }
      const long = vals.some(x => valueLabel(spec, x).length > 22);
      if (vals.length > 6) {
        return `<div class="st-row">${lbl}<select class="txt" data-select="${id}">${vals.map(x => `<option value="${esc(x)}" ${x === v ? "selected" : ""}>${esc(valueLabel(spec, x))}</option>`).join("")}</select></div>`;
      }
      if (long) {
        return `<div class="st-row col">${lbl}<div class="radio-list">${vals.map(x => `<button class="radio ${x === v ? "on" : ""}" data-set="${id}" data-val="${esc(x)}"><i></i>${esc(valueLabel(spec, x))}</button>`).join("")}</div></div>`;
      }
      return `<div class="st-row">${lbl}<div class="seg">${vals.map(x => `<button class="${x === v ? "on" : ""}" data-set="${id}" data-val="${esc(x)}">${esc(valueLabel(spec, x))}</button>`).join("")}</div></div>`;
    }
    case "number": {
      const { min, max, divisor } = spec.kind.data;
      const unit = divisor === 100 ? "%" : "";
      return `<div class="st-row">${lbl}<div class="rng"><input type="range" min="${min}" max="${max}" value="${esc(v)}" data-num="${id}" data-unit="${unit}"><output>${esc(v)}${unit}</output></div></div>`;
    }
    case "bool":
      return `<div class="st-row">${lbl}<label class="sw"><input type="checkbox" data-bool="${id}" ${v === "true" ? "checked" : ""}><i></i></label></div>`;
    case "color":
      return `<div class="st-row">${lbl}<div class="colorpick"><input type="color" value="${esc(v)}" data-color="${id}"><input class="txt" value="${esc(v)}" data-colortext="${id}" maxlength="7" spellcheck="false"></div></div>`;
    case "lines": {
      const presets = spec.module === "tmux" ? `<div class="bar presets">${TMUX_PRESETS.map(([t], i) => `<button class="chip" data-lines-preset="${id}" data-i="${i}">+ ${esc(t)}</button>`).join("")}</div>` : "";
      return `<div class="st-row col">${lbl}${presets}<textarea class="code-edit" rows="${Math.min(spec.key.startsWith("template_") ? 24 : 14, Math.max(4, v.split("\n").length + 1))}" data-lines="${id}" spellcheck="false" placeholder="${spec.key.startsWith("template_") ? "(sem template: o arquivo novo começa vazio)" : spec.module === "tmux" ? "bind -n M-z resize-pane -Z" : spec.module === "nvim" ? "vim.opt.spelllang = 'pt_br'" : "set spelllang=pt_br"}">${esc(v)}</textarea><small class="mut">Grava ao sair do campo.</small></div>`;
    }
    case "keymaps":
      return `<div class="st-row col">${lbl}${keymapsEditor(id, v)}</div>`;
    default:
      return `<div class="st-row">${lbl}<input class="txt" data-text="${id}" value="${esc(v)}" spellcheck="false"></div>`;
  }
}

function parseKeymaps(json) {
  try { const a = JSON.parse(json); return Array.isArray(a) ? a : []; } catch { return []; }
}
function keymapRow(k) {
  return `<div class="km-row"><select class="txt" data-k="mode">${MODE_LABELS.map(([m, t]) => `<option value="${m}" ${k.mode === m ? "selected" : ""}>${t}</option>`).join("")}</select>
    <input class="txt" data-k="lhs" value="${esc(k.lhs || "")}" placeholder="tecla, ex.: <leader>w" spellcheck="false">
    <input class="txt" data-k="rhs" value="${esc(k.rhs || "")}" placeholder="ação, ex.: <Cmd>w<CR>" spellcheck="false">
    <input class="txt" data-k="desc" value="${esc(k.desc || "")}" placeholder="descrição (opcional)">
    <button class="x" data-km-del title="Remover">✕</button></div>`;
}
function keymapsEditor(id, json) {
  const list = parseKeymaps(json);
  return `<div class="km" data-km="${id}">
    <div class="km-head"><span>Modo</span><span>Tecla</span><span>Ação</span><span>Descrição</span><span></span></div>
    <div class="km-rows">${list.map(keymapRow).join("") || ""}</div>
    <div class="km-empty mut" ${list.length ? "hidden" : ""}>Nenhum atalho seu ainda.</div>
    <div class="bar"><button class="hx-btn ghost sm" data-km-add>+ Adicionar atalho</button><select class="txt" data-km-preset><option value="">Prontos…</option>${KEYMAP_PRESETS.map(([t], i) => `<option value="${i}">${esc(t)}</option>`).join("")}</select></div>
    <small class="mut">Notação do Vim: <code>&lt;C-s&gt;</code> Ctrl+S · <code>&lt;A-x&gt;</code> Alt+X · <code>&lt;leader&gt;</code> sua tecla líder · <code>&lt;CR&gt;</code> Enter · <code>&lt;Cmd&gt;…&lt;CR&gt;</code> comando · <code>&lt;Nop&gt;</code> desativa a tecla.</small>
  </div>`;
}

function themePreviewHTML(p) {
  // Paleta de colorscheme (bg0, fg0, comment, red/green/amber/blue…) ou do kitty.
  const pick = (...names) => { for (const n of names) if (p[n]) return p[n]; };
  const bg = pick("bg0", "bg", "background", "bg1"), fg = pick("fg0", "fg", "foreground", "fg1");
  const kw = pick("red", "purple", "magenta", "pink", "keyword", "color5"), str = pick("green", "string", "color2");
  const fn = pick("blue", "cyan", "aqua", "func", "color4"), num = pick("amber", "orange", "yellow", "gold", "color3");
  const cm = pick("comment", "grey", "gray", "fg2", "color8") || fg;
  return { bg, html: `<pre style="color:${fg}"><span style="color:${cm}">-- ${"exemplo"}</span>\n<span style="color:${kw}">local</span> n = <span style="color:${num}">42</span>\n<span style="color:${fn}">print</span>(<span style="color:${str}">"oi"</span>)</pre>` };
}

/** Liga os controles de opção. `onChange(spec, valor|null)` grava (hub) ou edita o rascunho (assistente). */
function hydrateCustom(root, settings, onChange) {
  // A mesma raiz (#view) é redesenhada várias vezes: troca os ouvintes, não acumula.
  if (root._hydrated) root._hydrated.abort();
  const ctl = root._hydrated = new AbortController();
  const listen = (type, fn) => root.addEventListener(type, fn, { signal: ctl.signal });
  const specOf = id => { const [m, k] = id.split("."); return modInfo(m)?.options.find(o => o.key === k); };
  const term = mod => {
    const el = root.querySelector(`[data-term="${mod}"]`);
    if (!el) return;
    const m = modInfo(mod), get = k => optValue(m.options.find(o => o.key === k), settings);
    const pv = PREV[mod + ".palette"]?.[get("palette")] || {};
    const bg = pv.background || "#1d1c24", fg = pv.foreground || "#ddd", c = n => pv["color" + n] || fg;
    const op = Number(get("opacity")) / 100, bl = m.options.some(o => o.key === "blur") ? Number(get("blur")) : 0;
    el.style.background = `color-mix(in srgb,${bg} ${Math.round(op * 100)}%,transparent)`;
    el.style.backdropFilter = el.style.webkitBackdropFilter = `blur(${bl / 2}px)`;
    el.style.color = fg;
    el.innerHTML = `<div class="tb"><i></i><i></i><i></i></div><pre><span style="color:${c(2)};font-weight:700">ícaro</span> in <span style="color:${c(4)};font-weight:700">~/projetos</span> on <span style="color:${c(5)}"> main</span>
<span style="color:${c(2)}">❯</span> hiraku sync
<span style="color:${c(2)}">✓</span> kitty atualizado  <span style="color:${c(3)}">⚠ 1 conflito</span>  <span style="color:${c(1)}">✗ 0 erros</span>
<span style="color:${c(6)}">paleta</span> ${esc(get("palette"))} · opacidade ${esc(get("opacity"))}%</pre>`;
  };
  // Pré-visualizações: paletas do kitty (cores) e colorschemes (código colorido).
  $$("[data-pal]", root).forEach(async grid => {
    const [m, k] = grid.dataset.pal.split(".");
    const map = await previews(m, k);
    const code = grid.classList.contains("code");
    $$("[data-pv]", grid).forEach(el => {
      const p = map[el.dataset.pv];
      if (!p) { el.classList.add("none"); el.innerHTML = code ? `<span>${el.dataset.pv === "auto" ? "↺" : "Aa"}</span>` : ""; return; }
      if (code) {
        const t = themePreviewHTML(p);
        el.style.background = t.bg;
        el.innerHTML = t.html;
      } else {
        el.style.background = p.background;
        el.innerHTML = `<i style="background:${p.foreground};width:70%"></i><i style="background:${p.color4 || p.foreground};width:45%"></i><div class="dots">${[1, 2, 3, 4, 5, 6].map(n => `<i style="background:${p["color" + n]}"></i>`).join("")}</div>`;
      }
    });
    term(m);
  });
  const set = (id, val) => {
    const spec = specOf(id);
    if (!spec) return;
    const value = val === null || val === spec.default ? null : val;
    onChange(spec, value);
    term(spec.module);
  };
  const saveKeymaps = box => {
    const rows = $$(".km-row", box).map(r => {
      const g = k => r.querySelector(`[data-k="${k}"]`).value;
      const o = { mode: g("mode"), lhs: g("lhs").trim(), rhs: g("rhs") };
      if (g("desc").trim()) o.desc = g("desc").trim();
      return o;
    }).filter(k => k.lhs && k.rhs);
    $(".km-empty", box).hidden = $$(".km-row", box).length > 0;
    set(box.dataset.km, JSON.stringify(rows));
  };
  listen("click", e => {
    const b = e.target.closest("[data-set]");
    if (b) {
      b.parentElement.querySelectorAll("[data-set]").forEach(x => x.classList.toggle("on", x === b));
      set(b.dataset.set, b.dataset.val);
      return;
    }
    const r = e.target.closest("[data-reset]");
    if (r) { set(r.dataset.reset, null); rerenderCustom(root, settings, onChange); return; }
    const j = e.target.closest("[data-jump]");
    if (j) { $(`#cz-${j.dataset.jump}`)?.scrollIntoView({ behavior: "smooth", block: "start" }); $$("[data-jump]", root).forEach(x => x.classList.toggle("on", x === j)); return; }
    const add = e.target.closest("[data-km-add]");
    if (add) {
      const box = add.closest(".km");
      $(".km-rows", box).insertAdjacentHTML("beforeend", keymapRow({ mode: "n" }));
      $(".km-empty", box).hidden = true;
      $(".km-rows .km-row:last-child [data-k=lhs]", box).focus();
      return;
    }
    const del = e.target.closest("[data-km-del]");
    if (del) { const box = del.closest(".km"); del.closest(".km-row").remove(); saveKeymaps(box); return; }
    const lp = e.target.closest("[data-lines-preset]");
    if (lp) {
      const ta = root.querySelector(`[data-lines="${lp.dataset.linesPreset}"]`);
      const add = TMUX_PRESETS[Number(lp.dataset.i)][1];
      ta.value = (ta.value.trim() ? ta.value.replace(/\s*$/, "\n") : "") + add + "\n";
      set(lp.dataset.linesPreset, ta.value);
    }
  });
  listen("change", e => {
    const t = e.target;
    if (t.matches("[data-km-preset]")) {
      if (t.value === "") return;
      const box = t.closest(".km");
      $(".km-rows", box).insertAdjacentHTML("beforeend", keymapRow(KEYMAP_PRESETS[Number(t.value)][1]));
      t.value = "";
      saveKeymaps(box);
      return;
    }
    if (t.closest(".km-row")) { saveKeymaps(t.closest(".km")); return; }
    if (t.matches("[data-select]")) set(t.dataset.select, t.value);
    if (t.matches("[data-bool]")) set(t.dataset.bool, String(t.checked));
    if (t.matches("[data-text]")) set(t.dataset.text, t.value);
    if (t.matches("[data-lines]")) set(t.dataset.lines, t.value);
    if (t.matches("[data-colortext]")) {
      const v = t.value.trim().toLowerCase();
      if (/^#[0-9a-f]{6}$/.test(v)) { t.previousElementSibling.value = v; set(t.dataset.colortext, v); }
      else toast("Cor inválida: use #rrggbb", "err");
    }
  });
  listen("input", e => {
    const t = e.target;
    if (t.matches("[data-num]")) { t.nextElementSibling.textContent = t.value + t.dataset.unit; set(t.dataset.num, t.value); }
    if (t.matches("[data-color]")) { t.nextElementSibling.value = t.value; set(t.dataset.color, t.value); }
    if (t.matches("[data-filter-grid]")) {
      const q = t.value.trim().toLowerCase();
      $$(`[data-pal="${t.dataset.filterGrid}"] .th-card`, root).forEach(c => { c.hidden = q && !c.dataset.name.includes(q); });
    }
  });
  catalog().forEach(m => term(m.id));
}
function rerenderCustom(root, settings, onChange) {
  if (root.id === "view") render();
  else if (WZ) wzRender();
}
function setLocalOption(settings, spec, value) {
  settings.options = settings.options || {};
  const o = settings.options[spec.module] = settings.options[spec.module] || {};
  if (value === null) delete o[spec.key]; else o[spec.key] = value;
  if (!Object.keys(o).length) delete settings.options[spec.module];
}

// Hub: grava e aplica ao vivo (0,7 s depois da última mudança, só o módulo).
const APPLY = {};
const SAVING = {};
function liveChange(spec, value) {
  setLocalOption(SNAP.settings, spec, value);
  const id = spec.module + "." + spec.key;
  clearTimeout(SAVING[id]);
  SAVING[id] = setTimeout(async () => {
    try { await call("set_option", { module: spec.module, key: spec.key, value }); }
    catch (e) { fail(e); return; }
    if (!UI.live) return;
    if (!enabled(spec.module)) { toast(`${spec.module}: gravado. Habilite o módulo para aplicar.`); return; }
    if (!row(spec.module)?.installed) { toast(`${spec.module}: gravado. Vale na próxima sincronização.`); return; }
    clearTimeout(APPLY[spec.module]);
    APPLY[spec.module] = setTimeout(() => runSync(spec.module, { quiet: true }), 700);
  }, 150);
}
const CUSTOM_ORDER = ["kitty", "tmux", "nvim", "vim", "shell"];
let CZ_MOD = null;   // programa aberto em Personalizar
let CZ_SEC = {};     // seção aberta em cada programa
let CZ_Q = "";       // busca dentro do programa
function customMods() {
  const rank = id => { const i = CUSTOM_ORDER.indexOf(id); return i < 0 ? 99 : i; };
  return catalog().filter(m => m.supported && m.options.length).sort((a, b) => rank(a.id) - rank(b.id));
}
const changedCount = m => m.options.filter(o => optValue(o, SNAP.settings) !== o.default).length;
// ------------------------------------------------------------------ Descobrir
// Cada programa tem uma aba com extras: plugins (opções da seção "Descobrir", que
// começam desligadas) e programas do registry marcados com `discover = [<id>]`.
const DISC = "Descobrir";
const discoverMods = id => catalog().filter(x => x.supported && (x.discover || []).includes(id));
const linkBtn = url => url ? `<button class="dc-link" data-link="${esc(url)}" title="${esc(url)}">${esc(url.replace(/^https:\/\/(github\.com\/)?/, "").replace(/\/$/, ""))} ↗</button>` : "";
function discoverHTML(m, dopts, dmods) {
  const card = (on, head, desc, foot) => `<article class="dc-card ${on ? "on" : ""}">${head}<p>${esc(desc)}</p><div class="dc-foot">${foot}</div></article>`;
  const plugins = dopts.map(s => {
    const on = optValue(s, SNAP.settings) === "true";
    return card(on, `<div class="dc-top"><b>${esc(specLabel(s))}</b><label class="sw"><input type="checkbox" data-bool="${s.module}.${s.key}" ${on ? "checked" : ""}><i></i></label></div>`,
      s.description || "", linkBtn(s.link));
  }).join("");
  const tools = dmods.map(x => {
    const on = enabled(x.id), r = row(x.id), inst = r && r.installed;
    const st = inst ? `<span class="tag ok">instalado</span>` : on ? `<button class="hx-btn sm" data-install="${x.id}">Instalar agora</button>` : "";
    return card(on, `<div class="dc-top">${tile(x, "sm")}<b>${esc(x.name)}</b><label class="sw" title="${on ? "Desabilitar" : "Habilitar"} no sync"><input type="checkbox" data-toggle="${x.id}" ${on ? "checked" : ""}><i></i></label></div>`,
      x.description, `${st}${linkBtn(x.homepage)}`);
  }).join("");
  return `<p class="dc-intro mut">Extras que combinam com o ${esc(m.name)}. Ligue e o hiraku baixa e configura; desligar não apaga nada.</p>
    ${plugins ? `<div class="dc-sub">Plugins</div><div class="dc-grid">${plugins}</div>` : ""}
    ${tools ? `<div class="dc-sub">Programas</div><div class="dc-grid">${tools}</div>` : ""}`;
}

function viewCustom() {
  const mods = customMods();
  if (!mods.length) return `<div class="hx-wrap one"><div class="hx-panel mut">Nenhum módulo com opções.</div></div>`;
  if (!mods.some(m => m.id === CZ_MOD)) CZ_MOD = mods[0].id;
  const m = mods.find(x => x.id === CZ_MOD);
  const rail = mods.map(x => {
    const n = changedCount(x);
    return `<button class="cz-item ${x.id === CZ_MOD ? "on" : ""}" data-czmod="${x.id}">${tile(x, "sm")}<span><b>${esc(x.name)}</b><small>${x.options.length} opções${n ? ` · <em>${n} alterada${n > 1 ? "s" : ""}</em>` : ""}</small></span></button>`;
  }).join("");
  const specs = sortedSpecs([m]);
  const dmods = discoverMods(m.id);
  const dopts = specs.filter(s => s.section === DISC);
  const secs = sectionsOf(specs).filter(x => x !== DISC);
  if (dmods.length || dopts.length) secs.push(DISC);
  if (!secs.includes(CZ_SEC[m.id])) CZ_SEC[m.id] = secs[0];
  const q = CZ_Q.trim().toLowerCase();
  const shown = q
    ? specs.filter(s => (specLabel(s) + " " + s.key + " " + (s.description || "")).toLowerCase().includes(q))
    : specs.filter(s => (s.section || "") === CZ_SEC[m.id]);
  const tabs = secs.length > 1 && !q ? `<div class="cz-tabs" role="tablist">${secs.map(sec => {
    const n = specs.filter(s => (s.section || "") === sec && optValue(s, SNAP.settings) !== s.default).length
      + (sec === DISC ? dmods.filter(x => enabled(x.id)).length : 0);
    return `<button role="tab" class="${sec === CZ_SEC[m.id] ? "on" : ""} ${sec === DISC ? "disc" : ""}" data-czsec="${esc(sec)}">${sec === DISC ? "✦ " : ""}${esc(sec || "Geral")}${n ? `<i>${n}</i>` : ""}</button>`;
  }).join("")}</div>` : "";
  const hasTerm = m.options.some(o => o.key === "palette") && m.options.some(o => o.key === "opacity");
  const live = ["vim", "nvim", "tmux", "kitty"].includes(m.id) ? `<span class="tag acc" title="O programa aberto percebe a mudança e reaplica sozinho">aplica com ele aberto</span>` : "";
  const off = !enabled(m.id) ? `<span class="tag">desabilitado no sync</span>` : "";
  const body = !q && CZ_SEC[m.id] === DISC ? discoverHTML(m, dopts, dmods)
    : shown.length ? shown.map(s => optionRow(s, SNAP.settings)).join("")
    : `<div class="mut" style="padding:18px 0">Nenhuma opção com “${esc(CZ_Q)}”.</div>`;
  return `<div class="hx-wrap one"><div class="cz">
    <aside class="cz-rail glass" aria-label="Programas">
      <div class="cz-live"><label class="sw"><input type="checkbox" id="live" ${UI.live ? "checked" : ""}><i></i></label><span>Aplicar ao vivo</span></div>
      ${rail}</aside>
    <section class="cz-page">
      <header class="cz-head glass">${tile(m, "lg")}<div class="cz-title"><h2>${esc(m.name)}</h2><p class="mut">${esc(m.description)}</p><div class="cz-tags">${live}${off}</div></div>
        <input class="txt cz-search" id="czq" type="search" placeholder="Buscar em ${esc(m.name)}…" value="${esc(CZ_Q)}"></header>
      ${tabs}
      <div class="cz-sec glass" id="cz-${m.id}">
        ${hasTerm && (!q && (CZ_SEC[m.id] || "") === secs[0]) ? `<div class="termwrap"><div class="term" data-term="${m.id}"></div></div>` : ""}
        ${q ? `<div class="st-sec">Resultados em ${esc(m.name)}</div>` : ""}${body}
      </div>
    </section></div></div>`;
}

// ------------------------------------------------------------------ Conflitos
function viewConflicts() {
  const cs = SNAP.conflicts;
  const hero = `<section class="hx-hero sm"><div class="hx-logo">${applogo()}</div><div class="hx-hmain"><h2>Conflitos</h2><p class="mut">Arquivos que você editou e que o hiraku também quer mudar. Nada é sobrescrito sem você escolher.</p></div></section>`;
  if (!cs.length) return `<div class="hx-wrap one"><div class="hx-main">${hero}<div class="hx-sec">Pendentes</div><div class="hx-panel"><div class="hx-li"><span class="hx-dot ok">✓</span><div><b>Nenhum conflito</b><small>Tudo em ordem.</small></div></div></div></div></div>`;
  CONFLICT_SEL = Math.min(CONFLICT_SEL, cs.length - 1);
  const list = cs.map((c, i) => `<button class="cf-item ${i === CONFLICT_SEL ? "on" : ""}" data-cf="${i}"><b>${esc(c.module)}</b><small title="${esc(c.path)}">${esc(c.path)}</small></button>`).join("");
  const cur = cs[CONFLICT_SEL];
  return `<div class="hx-wrap one"><div class="hx-main">${hero}<div class="hx-sec">Pendentes <span class="n">${cs.length}</span></div>
    <div class="cf"><div class="cf-list glass">${list}</div><div class="glass" style="padding:14px">
    <div class="bar" style="justify-content:space-between"><b style="font:12px var(--mono)">${esc(cur.path)}</b><span><button class="hx-btn ghost sm" data-resolve="mine">Manter o meu</button> <button class="hx-btn sm" data-resolve="new">Usar o novo</button></span></div>
    <small class="mut">Diff: seu arquivo atual → versão nova do módulo. "Usar o novo" faz backup do seu antes.</small>
    <pre class="diff" id="diff">carregando…</pre></div></div></div></div>`;
}
async function loadDiff() {
  const el = $("#diff"), c = SNAP.conflicts[CONFLICT_SEL];
  if (!el || !c) return;
  try {
    const text = await call("diff", { path: c.path });
    el.innerHTML = (text.trim() ? text : "(sem diferenças)").split("\n").map(l => {
      const cls = l.startsWith("+++") || l.startsWith("---") ? "" : l.startsWith("+") ? "a" : l.startsWith("-") ? "d" : l.startsWith("@") ? "h" : "";
      return `<span class="${cls}">${esc(l) || " "}</span>`;
    }).join("");
  } catch (e) { el.textContent = "não foi possível gerar o diff: " + e; }
}

// ------------------------------------------------------------------ Atualizações
function viewUpdates() {
  const rep = SNAP.report, every = SNAP.scheduled ? UI.every : null;
  const seg = UPDATES.map(([v, t]) => `<button class="${(SNAP.scheduled ? v === every : v === null) ? "on" : ""}" data-every="${v ?? ""}">${t}</button>`).join("");
  const repRows = rep ? rep.modules.map(m => {
    const [ic, cls, txt] = STATUS[m.status] || ["•", "", m.status];
    const details = [m.error, ...m.configs.filter(c => c.outcome !== "Unchanged").map(c => `${c.outcome}: ${c.dest}`), ...m.hooks.map(h => `hook ${h.name}: ${h.outcome}`)].filter(Boolean);
    return `<div class="hx-li"><span class="hx-dot ${cls}">${ic}</span><div><b>${esc(m.id)} <span class="mut" style="font-weight:400">· ${esc(m.detail || txt)}</span></b>${details.map(d => `<small title="${esc(d)}">${esc(d)}</small>`).join("")}</div></div>`;
  }).join("") : `<small class="mut">Nenhum sync ainda.</small>`;
  return `<div class="hx-wrap"><div class="hx-main">
    <section class="hx-hero sm"><div class="hx-logo">${applogo()}</div><div class="hx-hmain"><h2>Atualizações</h2><p class="mut">Programas, configs do registry e o próprio app.</p>
    <div class="hx-cta"><button class="hx-btn" data-act="sync" ${SYNC ? "disabled" : ""}>${SYNC ? "Sincronizando…" : "Verificar e aplicar agora"}</button><button class="hx-btn ghost" data-act="appupdate">Atualizar o app</button></div></div></section>
    <div class="hx-sec">Atualização automática</div>
    <div class="glass" style="padding:6px 22px"><div class="st-row"><div class="st-lbl"><b>Rodar o sync sozinho</b><small>Em segundo plano (systemd/launchd/Agendador). Nunca pede senha nem sobrescreve arquivo seu: vira conflito.</small></div><div class="seg">${seg}</div></div></div>
    <div class="hx-sec">Último sync <span class="n">${rep ? ago(rep.finished_at) : "nunca"}</span></div>
    <div class="hx-panel">${rep && rep.notes.length ? rep.notes.map(n => `<div class="hx-li"><span class="hx-dot">i</span><div><small>${esc(n)}</small></div></div>`).join("") : ""}${repRows}</div></div>
    <aside class="hx-rail"><div class="hx-panel"><h6>Registry</h6><dl class="kv"><dt>versão</dt><dd>v${INFO.registry_version}</dd><dt>origem</dt><dd>${esc(INFO.registry_source)}</dd></dl>
    <div class="hx-cta" style="margin-top:10px"><button class="hx-btn ghost sm" data-act="registry">Buscar registry novo</button></div></div>
    <div class="hx-panel"><h6>Este app</h6><dl class="kv"><dt>versão</dt><dd>v${esc(INFO.version)}</dd><dt>automático</dt><dd>${UI.autoApp !== false ? "ligado" : "desligado"}</dd>${APP_UPDATE ? `<dt>nova</dt><dd>v${esc(APP_UPDATE.version)}${APP_UPDATE.ready ? " (baixada)" : ""}</dd>` : ""}</dl><div class="hx-cta" style="margin-top:10px"><button class="hx-btn ghost sm" data-act="appupdate">Verificar</button></div></div></aside></div>`;
}

// ------------------------------------------------------------------ eventos da página
function bindView() {
  const v = $("#view");
  v.onclick = async e => {
    const t = e.target.closest("[data-v],[data-act],[data-goto],[data-czmod],[data-czsec],[data-filter],[data-pin],[data-apply],[data-cf],[data-resolve],[data-every],[data-link],[data-install],[data-uninstall]");
    if (!t || !v.contains(t)) return;
    const d = t.dataset;
    if (d.v) return go(d.v);
    if (d.goto) { CZ_MOD = d.goto; CZ_Q = ""; return go("custom"); }
    if (d.czmod) { CZ_MOD = d.czmod; CZ_Q = ""; render(); $("#view").scrollTop = 0; return; }
    if (d.czsec !== undefined) { CZ_SEC[CZ_MOD] = d.czsec; render(); return; }
    if (d.filter) { MOD_FILTER = d.filter; return render(); }
    if (d.pin) return pinDialog(d.pin);
    if (d.apply) return runSync(d.apply);
    if (d.cf) { CONFLICT_SEL = Number(d.cf); return render(); }
    if (d.resolve) {
      const c = SNAP.conflicts[CONFLICT_SEL];
      try { await call("resolve", { path: c.path, keepMine: d.resolve === "mine" }); toast(d.resolve === "mine" ? "Mantido o seu arquivo." : "Versão nova aplicada (backup feito)."); }
      catch (err) { fail(err); }
      return reload();
    }
    if ("every" in d) return setSchedule(d.every ? Number(d.every) : null);
    if (d.act) return action(d.act);
    const lk = e.target.closest("[data-link]");
    if (lk) return call("open_url", { url: lk.dataset.link }).catch(fail);
    const un = e.target.closest("[data-uninstall]");
    if (un) return uninstallDialog([un.dataset.uninstall]);
    const ins = e.target.closest("[data-install]");
    if (ins) return runSync(ins.dataset.install);
  };
  v.oninput = e => {
    if (e.target.id !== "czq") return;
    CZ_Q = e.target.value;
    const pos = e.target.selectionStart;
    render();
    const q = $("#czq");
    if (q) { q.focus(); q.setSelectionRange(pos, pos); }
  };
  v.onchange = async e => {
    const dc = e.target.closest(".dc-card");
    if (dc && e.target.matches("[data-bool]")) dc.classList.toggle("on", e.target.checked);
    const tg = e.target.closest("[data-toggle]");
    if (tg) {
      try { const on = await call("toggle_module", { id: tg.dataset.toggle }); toast(`${tg.dataset.toggle}: ${on ? "habilitado (vale no próximo sync)" : "desabilitado (o que foi instalado continua)"}`); }
      catch (err) { fail(err); }
      return reload();
    }
    if (e.target.id === "live") { UI.live = e.target.checked; saveUI(); toast(UI.live ? "Aplicar ao vivo: ligado" : "Aplicar ao vivo: desligado — use Sincronizar"); }
  };
}
async function action(a) {
  switch (a) {
    case "sync": return runSync(null);
    case "wizard": return wzOpen(false);
    case "opendir": return call("open_config_dir").catch(fail);
    case "registry":
      try { const note = await call("refresh_registry"); toast(note || "O registry já está na versão mais nova (ou não há registry remoto configurado)."); }
      catch (e) { fail(e); }
      return reload();
    case "appupdate": return checkAppUpdate(true);
    case "health": return healthOpen();
    case "settings": return settingsOpen();
    case "about": return settingsOpen("about");
    case "keys": return settingsOpen("keys");
  }
}
async function setSchedule(every) {
  try {
    const note = await call("set_schedule", { every });
    if (every) { UI.every = every; saveUI(); }
    toast(note);
  } catch (e) { fail(e); }
  reload();
}

// ------------------------------------------------------------------ sync
async function runSync(only, opts = {}) {
  if (SYNC) { if (!opts.quiet) toast("Um sync já está em andamento."); return; }
  let roots = [];
  try { roots = await call("needs_root", { only }); } catch (e) { return fail(e); }
  if (roots.length) {
    if (opts.quiet) { toast(`${only} precisa de root para aplicar: use Sincronizar.`); return; }
    const ok = await sudoDialog(roots);
    if (!ok) return;
  }
  SYNC = { only, lines: [], progress: null, quiet: !!opts.quiet, repair: !!opts.repair };
  dock();
  try { await call("start_sync", { only, repair: !!opts.repair }); }
  catch (e) { SYNC = null; dock(); return fail(e); }
  $("#btn-sync").classList.add("spin");
  if (VIEW === "home" || VIEW === "updates") render();
}
function dock() {
  const d = $("#dock");
  if (!SYNC || (SYNC.quiet && !SYNC.lines.length)) { d.hidden = true; return; }
  d.hidden = false;
  d.className = "glass";
  const p = SYNC.progress;
  const pct = p && p.total ? Math.round(100 * p.done / p.total) : null;
  d.innerHTML = `<header><span class="hx-dot">⟳</span><b>${SYNC.repair ? "Reparando…" : SYNC.only ? `Aplicando ${esc(SYNC.only)}…` : "Sincronizando…"}</b><button class="x" id="dock-x" title="Esconder">–</button></header>
    <div class="pbar ${pct === null ? "ind" : ""}"><i style="width:${pct ?? 0}%"></i></div>
    <div class="log">${SYNC.lines.slice(-60).map(l => `<div class="${l.kind === "conflict" ? "c" : l.kind === "skipped" ? "s" : ""}">${esc(l.line)}</div>`).join("")}</div>`;
  const log = $(".log", d); log.scrollTop = log.scrollHeight;
  $("#dock-x").onclick = () => { d.hidden = true; };
}
function listenSync() {
on("sync://line", l => {
  if (!SYNC) return;
  if (l.kind === "progress") SYNC.progress = l; else SYNC.lines.push(l);
  if (!$("#dock").hidden || !SYNC.quiet) dock();
});
on("sync://done", r => {
  const was = SYNC;
  SYNC = null;
  $("#btn-sync").classList.remove("spin");
  $("#dock").hidden = true;
  (r.notes || []).forEach(n => toast(n));
  if (r.error) fail("Sync falhou: " + r.error);
  else if (r.report) {
    const f = r.report.modules.filter(m => m.status === "failed").length, c = r.report.modules.reduce((n, m) => n + m.configs.filter(x => x.outcome === "KeptPending" || /Pending|Conflict/.test(x.outcome)).length, 0);
    if (was && was.repair) { toast(f ? "Reparo terminou com falhas: veja a verificação." : "Reparado ✓ Os arquivos estão como o app manda.", f ? "err" : ""); setTimeout(() => healthOpen(), 400); }
    else if (was && was.only && !f) toast(`${was.only}: aplicado ✓`);
    else toast(`Sync concluído: ${r.report.modules.length} módulo(s)${f ? `, ${f} com falha` : ""}${c ? `, ${c} conflito(s)` : ""}.`, f ? "err" : "");
  }
  reload();
});
}

// ------------------------------------------------------------------ diálogos
function modal(html, cls = "") {
  const o = $("#overlay");
  o.innerHTML = `<div class="st-panel ${cls}" role="dialog" aria-modal="true">${html}</div>`;
  o.hidden = false;
  o.onclick = e => { if (e.target === o) closeModal(); };
  return o.firstElementChild;
}
function closeModal() { const o = $("#overlay"); o.hidden = true; o.innerHTML = ""; }
// ------------------------------------------------------------------ integridade
// Confere se o que o app deveria ter aplicado está no disco e se o vim/nvim/PATH
// são os certos. "Reparar" = sync que grava as configs exatamente como esperadas.
async function healthOpen() {
  const p = modal(`<div class="st-head"><h2>Verificar integridade</h2><button class="x" data-x>✕</button></div>
    <div class="st-pad hl"><p class="mut">Conferindo arquivos, programas e o PATH…</p><div class="pbar ind"><i></i></div></div>`, "wide");
  $$("[data-x]", p).forEach(b => b.onclick = closeModal);
  let r;
  try { r = await call("health"); } catch (e) { closeModal(); return fail(e); }
  const icon = l => l === "ok" ? "✓" : l === "warn" ? "!" : "✕";
  const row = it => `<div class="hl-it ${it.level}"><span class="hl-ic">${icon(it.level)}</span><div><b>${esc(it.title)}</b>${it.path ? `<code>${esc(it.path)}</code>` : ""}<small>${esc(it.detail)}</small></div></div>`;
  const mods = r.modules.map(m => {
    const bad = m.items.filter(i => i.level !== "ok"), good = m.items.filter(i => i.level === "ok");
    const st = bad.some(i => i.level === "error") ? "error" : bad.length ? "warn" : "ok";
    return `<section class="hl-mod ${st}"><header>${tile(modInfo(m.id) || { id: m.id, name: m.name }, "sm")}<b>${esc(m.name)}</b><span class="mut">${m.ok_files} arquivo(s) certo(s)${bad.length ? ` · ${bad.length} problema(s)` : ""}</span><span class="hl-ic ${st}">${icon(st)}</span></header>
      ${bad.map(row).join("")}${good.map(row).join("")}</section>`;
  }).join("");
  const sum = r.problems === 0 ? `<div class="hl-sum ok">✓ Tudo certo: os arquivos estão exatamente como o app manda.</div>`
    : `<div class="hl-sum">${r.problems} problema(s) encontrado(s)${r.fixable ? `; ${r.fixable} o Reparar resolve` : ""}.</div>`;
  p.innerHTML = `<div class="st-head"><h2>Verificar integridade</h2><button class="x" data-x>✕</button></div>
    <div class="st-pad hl">${sum}
      ${r.general.length ? `<section class="hl-mod"><header><b>Geral</b></header>${r.general.map(row).join("")}</section>` : ""}
      ${mods}
      <p class="mut small">Reparar grava de novo todos os arquivos do app exatamente como as suas opções mandam e roda os recarregamentos. O que estiver diferente (edições à mão, arquivos antigos, conflitos) vai antes para a pasta de backups do hiraku (<code>~/.local/share/hiraku/backups</code> no Linux).</p>
      <div class="hx-cta" style="justify-content:flex-end"><button class="hx-btn ghost" id="hl-again">Verificar de novo</button><button class="hx-btn" id="hl-fix" ${r.fixable ? "" : "disabled"}>Reparar${r.fixable ? ` (${r.fixable})` : ""}</button></div></div>`;
  $$("[data-x]", p).forEach(b => b.onclick = closeModal);
  $("#hl-again").onclick = () => healthOpen();
  $("#hl-fix").onclick = () => { closeModal(); runSync(null, { repair: true }); };
}

// ------------------------------------------------------------------ desinstalar
function reportList(reports) {
  const sec = (t, xs) => xs.length ? `<div class="un-sec"><b>${t} (${xs.length})</b><ul>${xs.slice(0, 40).map(x => `<li><code>${esc(x)}</code></li>`).join("")}${xs.length > 40 ? `<li class="mut">… e mais ${xs.length - 40}</li>` : ""}</ul></div>` : "";
  return reports.map(r => `<section class="hl-mod"><header>${tile(modInfo(r.module) || { id: r.module, name: r.module }, "sm")}<b>${esc((modInfo(r.module) || {}).name || r.module)}</b></header>
    ${sec("Apagados", r.removed)}${sec("Voltaram a ser o arquivo de antes do hiraku", r.restored)}${sec("Editados à mão (cópia no backup)", r.backed_up)}${sec("Mantidos (seus)", r.kept)}
    ${r.notes.map(n => `<div class="hl-it warn"><span class="hl-ic">!</span><div><small>${esc(n)}</small></div></div>`).join("")}
    ${!r.removed.length && !r.restored.length && !r.backed_up.length && !r.notes.length ? '<p class="mut small">Nada a desfazer.</p>' : ""}</section>`).join("");
}

function uninstallDialog(ids) {
  const all = !ids.length, name = all ? "tudo" : ((modInfo(ids[0]) || {}).name || ids[0]);
  const p = modal(`<div class="st-head"><h2>Desinstalar ${esc(name)}?</h2><button class="x" data-x>✕</button></div>
    <div class="st-pad"><ul class="un-what">
      <li>As configurações que o hiraku escreveu saem; os arquivos que você tinha antes do hiraku voltam.</li>
      <li>Arquivos editados à mão saem com uma cópia na pasta de backups.</li>
      <li>Plugins e programas baixados pelo hiraku (em ~/.local) são apagados.</li>
      <li>Pacotes do sistema (dnf/apt/brew) ficam; o resumo mostra o comando para tirar.</li>
      ${all ? "" : "<li>O módulo sai da lista de habilitados.</li>"}
    </ul>
    <div class="hx-cta" style="justify-content:flex-end"><button class="hx-btn ghost" data-x>Cancelar</button><button class="hx-btn danger" id="un-go">Desinstalar</button></div></div>`, "small");
  $$("[data-x]", p).forEach(b => b.onclick = closeModal);
  $("#un-go").onclick = async () => {
    $("#un-go").disabled = true; $("#un-go").textContent = "Desinstalando…";
    try {
      const reports = await call("uninstall", { ids });
      await reload();
      const q = modal(`<div class="st-head"><h2>Desinstalado</h2><button class="x" data-x>✕</button></div><div class="st-pad hl">${reportList(reports) || '<p class="mut">Nada a desfazer.</p>'}<div class="hx-cta" style="justify-content:flex-end"><button class="hx-btn" data-x>Fechar</button></div></div>`, "wide");
      $$("[data-x]", q).forEach(b => b.onclick = closeModal);
    } catch (e) { closeModal(); fail(e); }
  };
}

function uninstallAppDialog() {
  const p = modal(`<div class="st-head"><h2>Desinstalar o Hiraku Setup Manager?</h2><button class="x" data-x>✕</button></div>
    <div class="st-pad"><ul class="un-what">
      <li>Desinstala todos os programas e configurações do hiraku (os arquivos de antes dele voltam).</li>
      <li>Apaga as opções do app, o registry baixado e o sync agendado.</li>
      <li>Remove o app: o AppImage é apagado; o pacote .rpm/.deb pede a senha de administrador; no macOS vai para a Lixeira; no Windows abre o desinstalador.</li>
    </ul>
    <label class="chk"><input type="checkbox" id="un-keep" checked> Guardar os backups (arquivos antigos e editados)</label>
    <div class="hx-cta" style="justify-content:flex-end"><button class="hx-btn ghost" data-x>Cancelar</button><button class="hx-btn danger" id="un-go">Desinstalar tudo e o app</button></div></div>`, "small");
  $$("[data-x]", p).forEach(b => b.onclick = closeModal);
  $("#un-go").onclick = async () => {
    $("#un-go").disabled = true; $("#un-go").textContent = "Desinstalando…";
    try {
      const r = await call("uninstall_app", { keepBackups: $("#un-keep").checked });
      const q = modal(`<div class="st-head"><h2>${r.app.done ? "Pronto, até mais!" : "Quase lá"}</h2></div><div class="st-pad hl">
        <div class="hl-sum ${r.app.done ? "ok" : ""}">${esc(r.app.message)}</div>
        ${r.app.command ? `<p>Rode no terminal:</p><pre class="un-cmd"><code>${esc(r.app.command)}</code></pre>` : ""}
        ${reportList(r.reports)}
        <div class="hx-cta" style="justify-content:flex-end"><button class="hx-btn" id="un-quit">Fechar o app</button></div></div>`, "wide");
      $("#un-quit").onclick = () => call("quit").catch(() => window.close());
    } catch (e) { closeModal(); fail(e); }
  };
}

// "Lembrar o sudo": regra do sudoers que deixa só o install do dnf/apt/... sem senha.
function sudoRow() {
  if (INFO.os !== "linux" || !INFO.pkg_needs_root || INFO.is_root) return "";
  if (!INFO.can_root) return `<div class="st-row"><div class="st-lbl"><b>Senha do sudo</b><small>Esta máquina não tem sudo para você: use o modo "Só no meu usuário" e tudo vai para ~/.local, sem senha.</small></div></div>`;
  return `<div class="st-row"><div class="st-lbl"><b>Lembrar a senha do sudo</b><small>Deixa o hiraku instalar pacotes do sistema sem pedir senha. A senha não é guardada: o sudo passa a liberar só o <code>install</code> do gerenciador de pacotes (regra em /etc/sudoers.d). O resto do sudo continua pedindo senha.</small></div><label class="sw"><input type="checkbox" id="sudosave" ${INFO.sudo_saved ? "checked" : ""}><i></i></label></div>`;
}
function sudoDialog(mods, mode = {}) {
  const canSave = INFO.os === "linux" && INFO.pkg_needs_root && !INFO.sudo_saved && !mode.forget;
  const what = mode.forget ? "Para voltar a pedir a senha ao instalar, confirme uma última vez."
    : mods.length ? `Para instalar com o gerenciador de pacotes do sistema: <b>${esc(mods.join(", "))}</b>.`
    : "Para o hiraku instalar pacotes do sistema sem pedir senha de novo.";
  return new Promise(resolve => {
    const p = modal(`<div class="st-head"><h2>Senha de administrador</h2><button class="x" data-x>✕</button></div>
      <div class="st-pad"><p class="mut" style="margin-top:0">${what}</p>
      <input class="field" type="password" id="pw" placeholder="sua senha do sudo" autocomplete="current-password">
      ${canSave ? `<label class="chk"><input type="checkbox" id="pwsave" ${mode.remember ? "checked" : ""}> Lembrar: não pedir de novo nesta máquina</label>` : ""}
      <div class="err-t" id="pwerr"></div><small class="mut">A senha vai direto para o <code>sudo</code> e não é guardada. Prefere sem sudo? Mude o modo para "Só no meu usuário" nas Configurações.</small>
      <div class="hx-cta" style="justify-content:flex-end"><button class="hx-btn ghost" data-x>Cancelar</button><button class="hx-btn" id="pwok">Continuar</button></div></div>`, "small");
    const done = v => { closeModal(); resolve(v); };
    $$("[data-x]", p).forEach(b => b.onclick = () => done(false));
    const go = async () => {
      $("#pwok").disabled = true;
      const password = $("#pw").value, save = $("#pwsave")?.checked;
      try {
        if (save || mode.forget) toast(await call("sudo_remember", { password, on: !!save }));
        if (mods.length || !(save || mode.forget)) await call("sudo_auth", { password });
        if (save || mode.forget) INFO.sudo_saved = !!save;
        done(true);
      } catch (e) { $("#pwerr").textContent = String(e); $("#pwok").disabled = false; $("#pw").select(); }
    };
    $("#pwok").onclick = go;
    $("#pw").onkeydown = e => { if (e.key === "Enter") go(); if (e.key === "Escape") done(false); };
    setTimeout(() => $("#pw").focus(), 30);
  });
}
function pinDialog(id) {
  const cur = row(id)?.pin || "";
  const p = modal(`<div class="st-head"><h2>Fixar versão de ${esc(id)}</h2><button class="x" data-x>✕</button></div>
    <div class="st-pad"><p class="mut" style="margin-top:0">Trava o programa nessa versão (ex.: <code>0.10.*</code> ou <code>3.4</code>). As configs continuam sincronizando. Vazio = acompanhar o registry.</p>
    <input class="field" id="pin" value="${esc(cur)}" placeholder="ex.: 0.10.*" spellcheck="false">
    <div class="hx-cta" style="justify-content:flex-end">${cur ? '<button class="hx-btn ghost" id="unpin">Remover pin</button>' : ""}<button class="hx-btn ghost" data-x>Cancelar</button><button class="hx-btn" id="pinok">Salvar</button></div></div>`, "small");
  $$("[data-x]", p).forEach(b => b.onclick = closeModal);
  const save = async pin => { try { await call("set_pin", { module: id, pin }); toast(pin ? `${id}: fixado em ${pin}` : `${id}: volta a acompanhar o registry`); } catch (e) { fail(e); } closeModal(); reload(); };
  $("#pinok").onclick = () => save($("#pin").value.trim() || null);
  if ($("#unpin")) $("#unpin").onclick = () => save(null);
  $("#pin").onkeydown = e => { if (e.key === "Enter") $("#pinok").click(); };
  setTimeout(() => $("#pin").select(), 30);
}

// Configurações do app (mesmo modal do Comics Manager)
let ST_TAB = "look";
function settingsOpen(tab) {
  if (tab) ST_TAB = tab;
  const p = modal(`<div class="st-head"><h2>Configurações</h2><button class="x" data-x aria-label="Fechar">✕</button></div>
    <div class="st-main"><div class="st-rail">${[["look", "Aparência"], ["system", "Sistema"], ["keys", "Atalhos"], ["about", "Sobre"]].map(([k, t]) => `<button data-tab="${k}" class="${k === ST_TAB ? "on" : ""}">${t}</button>`).join("")}</div>
    <div class="st-body" id="stb"></div></div>`);
  $("[data-x]", p).onclick = closeModal;
  $$("[data-tab]", p).forEach(b => b.onclick = () => { ST_TAB = b.dataset.tab; settingsOpen(); });
  const body = $("#stb");
  if (ST_TAB === "look") {
    body.innerHTML = `<div class="st-sec">Tema</div><div class="th-grid">${THEMES.map(([k, n, bg, pn, ac]) => `<button class="th-card ${UI.theme === k ? "on" : ""}" data-theme="${k}"><div class="th-prev" style="background:${bg}"><i style="background:${pn};width:80%"></i><i style="background:${ac};width:25%"></i></div><span>${n}</span></button>`).join("")}</div>
      <div class="st-row"><div class="st-lbl"><b>Cor de destaque</b><small>Substitui a cor do tema</small></div><div class="acc-sw">${ACCS.map(c => `<button data-acc="${c}" class="${(UI.accent || "") === c ? "on" : ""}" style="background:${c || "conic-gradient(from 0deg,#c2577f,#7a6db0,#3a86c8,#3f9d77,#d9822b,#c2577f)"}" title="${c || "do tema"}"></button>`).join("")}<input type="color" id="accpick" value="${UI.accent || "#c2577f"}"></div></div>
      <div class="st-row"><div class="st-lbl"><b>Fonte dos títulos</b></div><div class="seg">${[["serif", "Serifada"], ["book", "Livro"], ["sans", "Sans"], ["mono", "Mono"]].map(([k, t]) => `<button data-font="${k}" class="${UI.font === k ? "on" : ""}">${t}</button>`).join("")}</div></div>`;
    body.onclick = e => {
      const t = e.target.closest("[data-theme],[data-acc],[data-font]");
      if (!t) return;
      if (t.dataset.theme) UI.theme = t.dataset.theme;
      if ("acc" in t.dataset) UI.accent = t.dataset.acc;
      if (t.dataset.font) UI.font = t.dataset.font;
      saveUI(); applyUI(); settingsOpen();
    };
    $("#accpick").oninput = e => { UI.accent = e.target.value; saveUI(); applyUI(); };
  } else if (ST_TAB === "system") {
    const m = SNAP.settings.mode || null;
    body.innerHTML = `<div class="st-sec">Instalação</div>
      <div class="st-row col"><div class="st-lbl"><b>Modo</b><small>Onde e como os programas são instalados.</small></div><div class="opt-grid">${MODES.map(([k, t, d, ic]) => `<button class="opt ${m === k ? "on" : ""}" data-mode="${k ?? ""}"><span class="ic">${ic}</span><b>${t}</b><small>${d}</small>${k === "system" && !INFO.can_root ? '<span class="warn">sudo não detectado nesta máquina</span>' : ""}</button>`).join("")}</div></div>
      ${sudoRow()}
      <div class="st-sec">Este app</div>
      <div class="st-row"><div class="st-lbl"><b>Atualizar o app sozinho</b><small>Procura versão nova ao abrir e a cada 6 h, baixa em segundo plano e só pede para reiniciar.</small></div><label class="sw"><input type="checkbox" id="autoapp" ${UI.autoApp !== false ? "checked" : ""}><i></i></label></div>
      <div class="st-row"><div class="st-lbl"><b>Arquivo de configurações</b><small style="font-family:var(--mono)">${esc(INFO.settings_file)}</small></div><button class="hx-btn ghost sm" id="od">Abrir pasta</button></div>
      <div class="st-row"><div class="st-lbl"><b>Refazer a instalação</b><small>Abre o assistente com as escolhas atuais.</small></div><button class="hx-btn ghost sm" id="rw">Abrir assistente</button></div>
      <div class="st-sec">Desinstalar</div>
      <div class="st-row"><div class="st-lbl"><b>Desinstalar tudo</b><small>Tira as configurações, plugins e programas que o hiraku instalou; os arquivos de antes do hiraku voltam. O app continua.</small></div><button class="hx-btn ghost sm danger" id="un-all">Desinstalar tudo</button></div>
      <div class="st-row"><div class="st-lbl"><b>Desinstalar o Hiraku Setup Manager</b><small>Desinstala tudo, apaga os dados do app (opções, registry, agendamento) e remove o próprio app.</small></div><button class="hx-btn ghost sm danger" id="un-app">Desinstalar o app</button></div>`;
    body.onclick = async e => {
      const t = e.target.closest("[data-mode]");
      if (t) { try { await call("set_mode", { mode: t.dataset.mode || null }); toast("Modo salvo. Vale no próximo sync."); } catch (err) { fail(err); } await reload(); settingsOpen(); }
    };
    const sw = $("#sudosave");
    if (sw) sw.onchange = async e => {
      const on = e.target.checked;
      e.target.checked = !on; // só muda de verdade se o sudo aceitar
      if (await sudoDialog([], { remember: on, forget: !on })) await reload();
      settingsOpen();
    };
    $("#un-all").onclick = () => uninstallDialog([]);
    $("#un-app").onclick = () => uninstallAppDialog();
    $("#od").onclick = () => call("open_config_dir").catch(fail);
    $("#autoapp").onchange = e => { UI.autoApp = e.target.checked; saveUI(); };
    $("#rw").onclick = () => { closeModal(); wzOpen(false); };
  } else if (ST_TAB === "keys") {
    body.innerHTML = `<div class="st-sec">Atalhos</div>${[["Ctrl+S", "Sincronizar tudo"], ["Ctrl+1…5", "Início, Módulos, Personalizar, Conflitos, Atualizações"], ["Ctrl+,", "Configurações"], ["Ctrl+Shift+L", "Alternar claro/escuro"], ["Esc", "Fechar janela"]].map(([k, t]) => `<div class="st-row"><span>${t}</span><kbd class="tag">${k}</kbd></div>`).join("")}`;
  } else {
    body.innerHTML = `<div style="display:flex;gap:18px;align-items:center;padding:20px 0"><div style="width:96px;height:96px">${applogo()}</div><div><h2 style="margin:0;font:700 24px var(--serif)">Hiraku Setup Manager</h2><p class="mut" style="margin:4px 0">v${esc(INFO.version)} · Criado por Ícaro Pinto Lira</p><p class="mut" style="margin:0">Instala kitty, tmux, Vim/Neovim, o shell e utilitários — e depois mantém tudo em dia e configurável.</p></div></div>
      <div class="st-row"><span>Registry</span><span class="mut">v${INFO.registry_version} (${esc(INFO.registry_source)})</span></div><div class="st-row"><span>Estado</span><span class="mut" style="font-family:var(--mono);font-size:12px">${esc(INFO.state_dir)}</span></div>
      <div class="hx-cta"><button class="hx-btn" id="au">Verificar atualizações</button></div>`;
    $("#au").onclick = () => checkAppUpdate(true);
  }
}

// Atualização do próprio app (tauri-plugin-updater, releases do GitHub).
// Automático: procura ao abrir e a cada 6 h, baixa em segundo plano e pergunta só
// na hora de reiniciar (no Windows o instalador fecha o app, então nunca instala sem
// você clicar).
let APP_UPDATE = null; // {update, version, ready} ou {manual: release do GitHub, version}
const REPO = "IcaropLira/HirakuSetupManager";
function isNewer(a, b) {
  const p = v => String(v).replace(/^v/i, "").split(/[.+-]/).map(n => parseInt(n, 10) || 0);
  const x = p(a), y = p(b);
  for (let i = 0; i < 3; i++) if ((x[i] || 0) !== (y[i] || 0)) return (x[i] || 0) > (y[i] || 0);
  return false;
}
async function latestRelease() {
  const r = await fetch(`https://api.github.com/repos/${REPO}/releases/latest`, { headers: { Accept: "application/vnd.github+json" } });
  if (!r.ok) throw new Error(`o GitHub respondeu ${r.status}`);
  return r.json();
}
// Sem latest.json (release publicada sem a assinatura do atualizador), o app não
// consegue se instalar sozinho: confere a última release e oferece o download.
async function manualCheck(manual) {
  let rel;
  try { rel = await latestRelease(); }
  catch (e) { if (manual) fail("Não consegui verificar atualizações: " + e.message); return; }
  if (!isNewer(rel.tag_name, INFO.version)) { if (manual) toast(`Você já está na versão mais nova (v${INFO.version}).`); return; }
  APP_UPDATE = { manual: rel, version: rel.tag_name.replace(/^v/i, "") };
  appUpdateBadge();
  if (manual) manualUpdateDialog(); else toast(`Versão ${APP_UPDATE.version} disponível — veja no rodapé.`);
}
function manualUpdateDialog() {
  const rel = APP_UPDATE.manual;
  const want = { linux: [/\.deb$/, /\.rpm$/, /\.AppImage$/], macos: [/\.dmg$/], windows: [/-setup\.exe$/, /\.msi$/] }[INFO.os] || [];
  const kind = n => /\.deb$/.test(n) ? "Ubuntu / Debian (.deb)" : /\.rpm$/.test(n) ? "Fedora (.rpm)" : /\.AppImage$/.test(n) ? "Qualquer Linux (.AppImage)"
    : /\.dmg$/.test(n) ? "macOS (.dmg)" : /\.exe$/.test(n) ? "Windows (instalador .exe)" : /\.msi$/.test(n) ? "Windows (.msi)" : n;
  const assets = (rel.assets || []).filter(a => want.some(r => r.test(a.name)));
  const p = modal(`<div class="st-head"><h2>Versão ${esc(APP_UPDATE.version)} disponível</h2><button class="x" data-x>✕</button></div>
    <div class="st-pad"><p style="margin-top:0">Você tem a ${esc(INFO.version)}. Esta versão ainda não se instala sozinha: baixe o instalador e abra — suas configurações continuam.</p>
    <div class="dl-list">${assets.map(a => `<button class="radio" data-url="${esc(a.browser_download_url)}"><i></i>${esc(kind(a.name))}<span class="mut">${(a.size / 1048576).toFixed(1)} MB</span></button>`).join("") || '<p class="mut">Nenhum instalador para este sistema nessa release.</p>'}</div>
    ${INFO.os === "linux" ? '<p class="mut" style="font-size:12px">Instalou pelo repositório do dnf? Então é só <code>sudo dnf upgrade hiraku-setup-manager</code>.</p>' : ""}
    <div class="hx-cta" style="justify-content:flex-end"><button class="hx-btn ghost" data-url="${esc(rel.html_url)}">Ver a release</button><button class="hx-btn" data-x>Fechar</button></div></div>`, "small");
  $$("[data-x]", p).forEach(b => b.onclick = closeModal);
  $$("[data-url]", p).forEach(b => b.onclick = () => call("open_url", { url: b.dataset.url }).then(() => toast("Abrindo no navegador…")).catch(fail));
}
async function checkAppUpdate(manual) {
  const U = T && T.updater;
  if (!U) { if (manual) toast("A atualização do app só funciona no app instalado."); return; }
  if (APP_UPDATE && APP_UPDATE.manual) return manual ? manualUpdateDialog() : undefined;
  if (APP_UPDATE && APP_UPDATE.ready) return appUpdateReady();
  let up;
  try { up = await U.check(); }
  catch (e) { return manualCheck(manual); }
  if (!up) { if (manual) toast("Você já está na versão mais nova."); return; }
  APP_UPDATE = { update: up, version: up.version, ready: false };
  if (!manual && UI.autoApp !== false) {
    try {
      await up.download();
      APP_UPDATE.ready = true;
      appUpdateBadge();
      toast(`Versão ${up.version} baixada. Reinicie quando quiser (rodapé).`);
    } catch (e) { APP_UPDATE = null; }
    return;
  }
  appUpdateDialog();
}
function appUpdateBadge() {
  const a = $("#chkup");
  a.textContent = APP_UPDATE.manual ? `Baixar a v${APP_UPDATE.version}` : `Reiniciar para atualizar (v${APP_UPDATE.version})`;
  a.classList.add("upready");
}
function appUpdateReady() {
  const p = modal(`<div class="st-head"><h2>Atualização pronta</h2><button class="x" data-x>✕</button></div><div class="st-pad"><p>A versão <b>${esc(APP_UPDATE.version)}</b> já foi baixada (você tem ${esc(INFO.version)}).</p>${APP_UPDATE.update.body ? `<p class="mut">${esc(APP_UPDATE.update.body)}</p>` : ""}<div class="hx-cta" style="justify-content:flex-end"><button class="hx-btn ghost" data-x>Depois</button><button class="hx-btn" id="upok">Reiniciar e atualizar</button></div></div>`, "small");
  $$("[data-x]", p).forEach(b => b.onclick = closeModal);
  $("#upok").onclick = async () => {
    if (SYNC) { toast("Espere o sync terminar."); return; }
    $("#upok").disabled = true;
    try { await APP_UPDATE.update.install(); await T.process.relaunch(); }
    catch (e) { $("#upok").disabled = false; fail("Falha ao instalar: " + e); }
  };
}
function appUpdateDialog() {
  const up = APP_UPDATE.update;
  const p = modal(`<div class="st-head"><h2>Atualização disponível</h2><button class="x" data-x>✕</button></div><div class="st-pad"><p>Versão <b>${esc(up.version)}</b> (você tem ${esc(INFO.version)}).</p>${up.body ? `<p class="mut">${esc(up.body)}</p>` : ""}<div class="pbar" id="upb" hidden><i style="width:0"></i></div><div class="hx-cta" style="justify-content:flex-end"><button class="hx-btn ghost" data-x>Depois</button><button class="hx-btn" id="upok">Baixar e reiniciar</button></div></div>`, "small");
  $$("[data-x]", p).forEach(b => b.onclick = closeModal);
  $("#upok").onclick = async () => {
    $("#upok").disabled = true; $("#upb").hidden = false;
    let total = 0, done = 0;
    try {
      await up.download(ev => {
        if (ev.event === "Started") total = ev.data.contentLength || 0;
        if (ev.event === "Progress") { done += ev.data.chunkLength; if (total) $("#upb i").style.width = (100 * done / total) + "%"; }
      });
      APP_UPDATE.ready = true;
      await up.install();
      await T.process.relaunch();
    } catch (e) { $("#upok").disabled = false; fail("Falha na atualização: " + e); }
  };
}

// ------------------------------------------------------------------ assistente
let WZ = null;
const WZ_STEPS = [["mode", "Permissões"], ["editor", "Editor"], ["modules", "O que instalar"], ["look", "Aparência"], ["updates", "Atualizações"], ["review", "Revisar e instalar"]];
function wzOpen(first) {
  const s = JSON.parse(JSON.stringify(SNAP.settings));
  const sup = catalog().filter(m => m.supported).map(m => m.id);
  if (first) s.modules = RECOMMENDED.filter(id => sup.includes(id));
  WZ = { step: 0, first, draft: s, every: first ? 86400 : SNAP.scheduled ? UI.every : null };
  let w = $("#wz");
  if (!w) { w = document.createElement("div"); w.id = "wz"; w.className = "wz"; document.body.appendChild(w); }
  wzRender();
}
function wzClose() { WZ = null; $("#wz")?.remove(); }
function wzSteps() { return WZ_STEPS.filter(([k]) => k !== "look" || catalog().some(m => withDeps(WZ.draft.modules).includes(m.id) && m.options.length)); }
function wzRender() {
  const w = $("#wz"), steps = wzSteps(), cur = steps[WZ.step][0], d = WZ.draft;
  const rail = `<div class="wz-rail"><div class="lg">${applogo()}</div><h1>${WZ.first ? "Bem-vindo ao Hiraku" : "Refazer instalação"}</h1>
    ${steps.map(([k, t], i) => `<button class="wz-step ${i === WZ.step ? "on" : i < WZ.step ? "done" : ""}" data-wzgo="${i}"><i>${i < WZ.step ? "✓" : i + 1}</i>${t}</button>`).join("")}
    <div class="foot">${esc(INFO.env_label)}<br>Nada é gravado até o último passo.</div></div>`;
  let title = "", sub = "", body = "";
  if (cur === "mode") {
    title = WZ.first ? "Vamos montar seu setup" : "Permissões";
    sub = "kitty, tmux, Vim/Neovim, o shell e utilitários — instalados agora e mantidos em dia depois. Primeiro: você tem acesso a sudo nesta máquina?";
    body = `<div class="opt-grid">${MODES.map(([k, t, dd, ic]) => `<button class="opt ${(d.mode || null) === k ? "on" : ""}" data-wzmode="${k ?? ""}"><span class="ic">${ic}</span><b>${t}</b><small>${dd}</small>${k === "system" && !INFO.can_root ? '<span class="warn">sudo não detectado</span>' : ""}</button>`).join("")}</div>`;
  } else if (cur === "editor") {
    title = "Qual editor?"; sub = "Os dois usam os mesmos atalhos, temas e templates.";
    const has = id => d.modules.includes(id);
    const sel = EDITORS.findIndex(([, ids]) => ids.length === ["vim", "nvim"].filter(has).length && ids.every(has));
    body = `<div class="opt-grid">${EDITORS.map(([t, ids, dd, ic], i) => `<button class="opt ${i === sel ? "on" : ""}" data-wzed="${i}"><span class="ic">${ic}</span><b>${t}</b><small>${dd}</small></button>`).join("")}</div>`;
  } else if (cur === "modules") {
    const mods = catalog().filter(m => m.supported);
    const all = mods.every(m => d.modules.includes(m.id));
    title = "O que instalar?"; sub = `${d.modules.length} marcados. Dependências entram sozinhas.`;
    body = `<div class="bar"><button class="chip" data-wzall>${all ? "Desmarcar tudo" : "Marcar tudo"}</button><button class="chip" data-wzrec>Recomendado</button></div>
      <div class="mod-grid">${mods.map(m => `<button class="mod ${d.modules.includes(m.id) ? "on" : ""}" data-wzmod="${m.id}"><span class="ck">${d.modules.includes(m.id) ? "✓" : ""}</span><span><b>${esc(m.name)}</b><small>${esc(m.description)}</small>${m.requires.length ? `<span class="dep">+ ${esc(m.requires.join(", "))}</span>` : ""}</span></button>`).join("")}</div>`;
  } else if (cur === "look") {
    title = "Deixe com a sua cara"; sub = "Dá para mudar tudo isso depois em Personalizar — e ver na hora.";
    const ids = withDeps(d.modules);
    body = customHTML(catalog().filter(m => ids.includes(m.id)), d, { wizard: true });
  } else if (cur === "updates") {
    title = "Atualizações automáticas"; sub = "O hiraku pode buscar versões novas dos programas e das configs sozinho, em segundo plano.";
    body = `<div class="opt-grid">${UPDATES.map(([v, t, dd]) => `<button class="opt ${WZ.every === v ? "on" : ""}" data-wzevery="${v ?? ""}"><span class="ic">${v ? "↻" : "✋"}</span><b>${t}</b><small>${dd}</small></button>`).join("")}</div>
      <p class="mut" style="max-width:720px">Programas com versão fixada não mudam. Se você editou um arquivo que o hiraku também quer mudar, ele vira um conflito para você decidir — nada é sobrescrito.</p>`;
  } else {
    title = "Tudo pronto"; sub = "Confira e instale. Depois disso esta janela vira o seu hub de configuração.";
    const ids = withDeps(d.modules), extra = ids.filter(i => !d.modules.includes(i));
    const opts = Object.entries(d.options || {}).flatMap(([m, o]) => Object.entries(o).map(([k, v]) => `${m}.${k} = ${v}`));
    body = `<dl class="sum glass"><dt>Modo</dt><dd>${MODES.find(x => x[0] === (d.mode || null))[1]}</dd>
      <dt>Módulos</dt><dd>${d.modules.length ? esc(d.modules.join(", ")) : '<span class="warn">nenhum — volte e escolha algo</span>'}${extra.length ? `<br><small class="mut">+ dependências: ${esc(extra.join(", "))}</small>` : ""}</dd>
      <dt>Personalização</dt><dd>${opts.length ? esc(opts.join(" · ")) : "padrões"}</dd>
      <dt>Atualizações</dt><dd>${UPDATES.find(x => x[0] === WZ.every)[1]}</dd>
      <dt>Arquivo</dt><dd style="font:12px var(--mono)">${esc(INFO.settings_file)}</dd></dl>`;
  }
  const last = WZ.step === steps.length - 1;
  w.innerHTML = `${rail}<div class="wz-main"><h2>${title}</h2><p>${sub}</p><div class="wz-body" id="wzb">${body}</div>
    <div class="wz-foot"><span class="sp">Passo ${WZ.step + 1} de ${steps.length}</span>
    <button class="hx-btn ghost" id="wzx">${WZ.first ? "Pular por agora" : "Cancelar"}</button>
    ${WZ.step ? '<button class="hx-btn ghost" id="wzb-back">Voltar</button>' : ""}
    <button class="hx-btn" id="wzn" ${last && !d.modules.length ? "disabled" : ""}>${last ? "Instalar agora" : "Continuar"}</button></div></div>`;
  if (cur === "look") hydrateCustom($("#wzb"), d, (spec, v) => setLocalOption(d, spec, v));
  w.onclick = e => {
    const t = e.target.closest("[data-wzgo],[data-wzmode],[data-wzed],[data-wzmod],[data-wzall],[data-wzrec],[data-wzevery]");
    if (!t) return;
    const x = t.dataset;
    if (x.wzgo !== undefined) { if (Number(x.wzgo) < WZ.step) { WZ.step = Number(x.wzgo); wzRender(); } return; }
    if (x.wzmode !== undefined) d.mode = x.wzmode || undefined;
    if (x.wzed !== undefined) { const want = EDITORS[Number(x.wzed)][1]; ["vim", "nvim"].forEach(id => { d.modules = d.modules.filter(m => m !== id); if (want.includes(id) && modInfo(id)?.supported) d.modules.push(id); }); }
    if (x.wzmod) d.modules = d.modules.includes(x.wzmod) ? d.modules.filter(m => m !== x.wzmod) : [...d.modules, x.wzmod];
    if (x.wzall !== undefined) { const mods = catalog().filter(m => m.supported).map(m => m.id); d.modules = mods.every(id => d.modules.includes(id)) ? [] : mods; }
    if (x.wzrec !== undefined) d.modules = RECOMMENDED.filter(id => modInfo(id)?.supported);
    if (x.wzevery !== undefined) WZ.every = x.wzevery ? Number(x.wzevery) : null;
    wzRender();
  };
  $("#wzx").onclick = () => { wzClose(); if (WZ === null) toast(INFO.first_run ? "Assistente fechado sem gravar. Reabra em Ir → Refazer instalação." : "Nada mudou."); };
  if ($("#wzb-back")) $("#wzb-back").onclick = () => { WZ.step--; wzRender(); };
  $("#wzn").onclick = async () => {
    if (!last) { WZ.step++; wzRender(); $(".wz-main").scrollTop = 0; return; }
    const settings = JSON.parse(JSON.stringify(d));
    if (!settings.mode) delete settings.mode;
    $("#wzn").disabled = true;
    try {
      await call("save_settings", { settings });
      const note = await call("set_schedule", { every: WZ.every }).catch(e => "agendamento não configurado: " + e);
      if (WZ.every) { UI.every = WZ.every; saveUI(); }
      toast("Configurações gravadas · " + note);
    } catch (e) { $("#wzn").disabled = false; return fail(e); }
    wzClose();
    await reload();
    go("home");
    runSync(null);
  };
}

// ------------------------------------------------------------------ menus e atalhos
const MENUS = {
  file: [["Sincronizar tudo", "Ctrl+S", () => runSync(null)], ["Verificar integridade", "Ctrl+I", () => healthOpen()], ["Buscar registry novo", "", () => action("registry")], "-", ["Abrir pasta de configurações", "", () => action("opendir")], "-", ["Sair", "Ctrl+Q", () => T && T.process ? T.process.exit(0) : window.close()]],
  edit: [["Configurações…", "Ctrl+,", () => settingsOpen()]],
  view: [["Início", "Ctrl+1", () => go("home")], ["Módulos", "Ctrl+2", () => go("modules")], ["Personalizar", "Ctrl+3", () => go("custom")], ["Conflitos", "Ctrl+4", () => go("conflicts")], ["Atualizações", "Ctrl+5", () => go("updates")], "-", ["Alternar claro/escuro", "Ctrl+Shift+L", toggleDark]],
  go: [["Refazer instalação…", "", () => wzOpen(false)], ["Arquivo settings.toml", "", () => action("opendir")]],
  help: [["Verificar integridade (nada aplica?)", "Ctrl+I", () => healthOpen()], ["Desinstalar…", "", () => settingsOpen("system")], ["Atalhos de teclado", "", () => settingsOpen("keys")], ["Verificar atualizações", "", () => checkAppUpdate(true)], "-", ["Sobre o Hiraku Setup", "", () => settingsOpen("about")]],
};
function buildMenus() {
  $$(".menubar .nvm").forEach(m => {
    m.innerHTML = MENUS[m.dataset.m].map((it, i) => it === "-" ? "<hr>" : `<button class="mbi" data-i="${i}"><span>${it[0]}</span><kbd>${it[1]}</kbd></button>`).join("");
    m.onclick = e => { const b = e.target.closest(".mbi"); if (!b) return; closeMenus(); MENUS[m.dataset.m][b.dataset.i][2](); };
  });
  $$(".menubar .nvt").forEach(b => {
    b.onclick = e => { e.stopPropagation(); const g = b.parentElement, was = g.classList.contains("open"); closeMenus(); if (!was) g.classList.add("open"); };
    b.onmouseenter = () => { if ($(".menubar .open") && !b.parentElement.classList.contains("open")) { closeMenus(); b.parentElement.classList.add("open"); } };
  });
  document.addEventListener("click", closeMenus);
}
const closeMenus = () => $$(".nvg.open").forEach(g => g.classList.remove("open"));
function toggleDark() {
  const dark = UI.theme === "auto" ? matchMedia("(prefers-color-scheme: dark)").matches : ["dark", "midnight", "ocean", "forest", "nord", "dracula", "mocha", "sunset"].includes(UI.theme);
  UI.theme = dark ? "light" : "dark"; saveUI(); applyUI();
}
document.addEventListener("keydown", e => {
  if (e.key === "Escape") { if (!$("#overlay").hidden) closeModal(); else closeMenus(); return; }
  if (!(e.ctrlKey || e.metaKey)) return;
  const k = e.key.toLowerCase();
  const map = { s: () => runSync(null), i: () => healthOpen(), ",": () => settingsOpen(), 1: () => go("home"), 2: () => go("modules"), 3: () => go("custom"), 4: () => go("conflicts"), 5: () => go("updates") };
  if (e.shiftKey && k === "l") { e.preventDefault(); return toggleDark(); }
  if (map[k] && !WZ) { e.preventDefault(); map[k](); }
});

// ------------------------------------------------------------------ início
async function boot() {
  applyUI();
  $(".brand-mark").innerHTML = applogo();
  buildMenus();
  listenSync();
  $$("nav button[data-v], .brand-mark").forEach(b => b.onclick = () => go(b.dataset.v));
  $("#btn-sync").onclick = () => runSync(null);
  $("#btn-theme").onclick = toggleDark;
  $("#btn-settings").onclick = () => settingsOpen();
  $("#chkup").onclick = e => { e.preventDefault(); checkAppUpdate(true); };
  await reload();
  if (!INFO) return;
  if (catalog().some(m => m.id === "kitty")) previews("kitty", "palette").then(() => { if (VIEW === "home") render(); });
  if (INFO.first_run) wzOpen(true);
  setTimeout(() => checkAppUpdate(false), 4000);
  setInterval(() => checkAppUpdate(false), 6 * 3600 * 1000);
}

// ------------------------------------------------------------------ dados de exemplo (navegador sem Tauri)
const MOCK = {
  listeners: {},
  settings: null,
  first: true,
  pal: { navy: ["#0b1220", "#dbeafe", ["#1e293b", "#f87171", "#4ade80", "#facc15", "#60a5fa", "#c084fc", "#22d3ee", "#e2e8f0"]], nord: ["#2e3440", "#d8dee9", ["#3b4252", "#bf616a", "#a3be8c", "#ebcb8b", "#81a1c1", "#b48ead", "#88c0d0", "#e5e9f0"]], dracula: ["#282a36", "#f8f8f2", ["#21222c", "#ff5555", "#50fa7b", "#f1fa8c", "#bd93f9", "#ff79c6", "#8be9fd", "#f8f8f2"]], gruvbox: ["#282828", "#ebdbb2", ["#282828", "#cc241d", "#98971a", "#d79921", "#458588", "#b16286", "#689d6a", "#a89984"]], catppuccin: ["#1e1e2e", "#cdd6f4", ["#45475a", "#f38ba8", "#a6e3a1", "#f9e2af", "#89b4fa", "#f5c2e7", "#94e2d5", "#bac2de"]], purple: ["#17111f", "#e9d5ff", ["#2e1f42", "#f472b6", "#86efac", "#fde68a", "#a78bfa", "#e879f9", "#67e8f9", "#ede9fe"]], solarized_light: ["#fdf6e3", "#586e75", ["#073642", "#dc322f", "#859900", "#b58900", "#268bd2", "#d33682", "#2aa198", "#eee8d5"]], sunset: ["#1c1419", "#f6e8e0", ["#3b2a31", "#ff6b6b", "#c3e88d", "#ffcb6b", "#ff8a5b", "#f78c6c", "#89ddff", "#f6e8e0"]]},
  cat() {
    const o = (key, kind, def, description) => ({ module: "kitty", key, kind, default: def, description });
    const m = (id, name, description, requires = [], options = []) => ({ id, name, description, supported: true, requires, options });
    const list = [
      m("bat", "bat", "Substituto do cat com realce de sintaxe"),
      m("font-jetbrains-mono", "JetBrainsMono Nerd Font", "Fonte com ícones (Nerd Font) usada pelo kitty, vim e nvim"),
      m("icaro-themes", "Temas do Ícaro", "Colorschemes compartilhados pelo vim e pelo nvim (90 arquivos)"),
      m("kitty", "kitty", "Terminal com aceleração de GPU, com paleta e transparência configuráveis", ["font-jetbrains-mono"], [o("palette", { type: "choice", data: Object.keys(this.pal) }, "navy", "Paleta de cores (arquivos em files/palettes)"), o("opacity", { type: "number", data: { min: 30, max: 100, divisor: 100 } }, "80", "Opacidade do fundo, em % (100 = sólido)"), o("blur", { type: "number", data: { min: 0, max: 64, divisor: 1 } }, "18", "Desfoque do fundo (0 = desligado)")]),
      m("nvim", "Neovim", "Neovim com lazy.nvim, LSP, temas do Ícaro e autocomplete", ["icaro-themes", "font-jetbrains-mono"]),
      m("shell", "Shell (bash/zsh)", "Bloco no .bashrc/.zshrc, aliases, starship e o visualizer de boas-vindas", [], [{ module: "shell", key: "visualizer", kind: { type: "choice", data: ["none", "fastfetch", "neofetch"] }, default: "none", description: "Programa exibido ao abrir um shell" }, { module: "shell", key: "fastfetch_color", kind: { type: "text" }, default: "@33", description: "Cor do fastfetch (código do --color)" }]),
      m("starship", "starship", "Prompt rápido e configurável"),
      m("tmux", "tmux", "Multiplexador de terminal (prefixo Ctrl+A)"),
      m("vim", "Vim", "Vim com tema, plugins (airline, nerdtree, coc...) e wrapper", ["icaro-themes", "font-jetbrains-mono"]),
    ].map(x => (window.MOCK_EXTRA && window.MOCK_EXTRA.catalog[x.id]) || x);
    if (window.MOCK_EXTRA) list.push(...Object.values(window.MOCK_EXTRA.catalog).filter(x => !list.some(y => y.id === x.id)));
    return list;
  },
  report: null,
  async call(cmd, a) {
    await new Promise(r => setTimeout(r, 30));
    if (!this.settings) this.settings = { schema: 1, modules: [], pins: {}, options: {} };
    const s = this.settings;
    switch (cmd) {
      case "info": return { version: "0.1.0", env_label: "linux · fedora · x86_64 · sudo disponível", os: "linux", can_root: true, is_root: false, sudo_saved: !!this.sudoSaved, pkg_needs_root: true, first_run: this.first && !new URLSearchParams(location.search).has("hub"), registry_version: 1, registry_source: "embedded", settings_file: "/home/icaro/.config/hiraku/settings.toml", state_dir: "/home/icaro/.local/state/hiraku", running: false };
      case "snapshot": {
        if (new URLSearchParams(location.search).has("hub") && !s.modules.length) { s.modules = ["kitty", "font-jetbrains-mono", "shell", "nvim", "tmux"]; s.options = { kitty: { palette: "nord" } }; this.first = false; this.report = this.fakeReport(); }
        const cat = this.cat();
        return {
          catalog: cat, settings: JSON.parse(JSON.stringify(s)), scheduled: this.sched || false, report: this.report,
          conflicts: s.modules.includes("shell") ? [{ path: "/home/icaro/.bashrc", module: "shell" }] : [],
          modules: cat.map(m => ({ id: m.id, description: m.description, enabled: s.modules.includes(m.id), installed: this.report && s.modules.includes(m.id) ? ({ kitty: "0.39.1", tmux: "3.5a", nvim: "0.11.2" }[m.id] || "configs") : null, pin: s.pins[m.id] || null, supported: true })),
        };
      }
      case "previews": if (window.MOCK_EXTRA && window.MOCK_EXTRA.previews[a.module + "." + a.key]) return window.MOCK_EXTRA.previews[a.module + "." + a.key];
        return Object.fromEntries(Object.entries(this.pal).map(([k, [bg, fg, c]]) => [k, Object.assign({ background: bg, foreground: fg }, ...c.map((x, i) => ({ ["color" + i]: x })))]));
      case "toggle_module": { const on = !s.modules.includes(a.id); s.modules = on ? [...s.modules, a.id] : s.modules.filter(m => m !== a.id); return on; }
      case "save_settings": this.settings = a.settings; this.first = false; return null;
      case "set_option": { s.options[a.module] = s.options[a.module] || {}; if (a.value === null) delete s.options[a.module][a.key]; else s.options[a.module][a.key] = a.value; return null; }
      case "set_pin": if (a.pin) s.pins[a.module] = a.pin; else delete s.pins[a.module]; return null;
      case "set_mode": s.mode = a.mode || undefined; return null;
      case "set_schedule": this.sched = !!a.every; return a.every ? `atualização automática a cada ${a.every / 3600}h` : "atualização manual";
      case "diff": return "--- atual\n+++ nova\n@@ -1,4 +1,4 @@\n # >>> ícaro-lira >>>\n-alias ll='ls -la'\n+alias ll='eza -la --icons'\n source ~/.config/dotfiles-shell/shellrc.sh\n # <<< ícaro-lira <<<";
      case "resolve": return null;
      case "needs_root": return [];
      case "sudo_auth": return null;
      case "uninstall": { const ids = a.ids.length ? a.ids : this.settings.modules.slice(); this.settings.modules = this.settings.modules.filter(m => !ids.includes(m));
        return ids.map(id => ({ module: id, removed: [`/home/icaro/.config/${id}/a.conf`, `/home/icaro/.local/share/hiraku/${id}`], restored: id === "vim" ? ["/home/icaro/.vimrc"] : [], backed_up: [], kept: id === "vim" ? ["/home/icaro/.vim/config/local.vim"] : [], notes: id === "tmux" ? ["tmux veio do dnf e continua no sistema (outros programas podem usar). Para tirar: sudo dnf remove tmux"] : [] })); }
      case "uninstall_app": return { reports: [{ module: "vim", removed: ["/home/icaro/.vim/config/hiraku.vim"], restored: ["/home/icaro/.vimrc"], backed_up: [], kept: [], notes: [] }], app: { kind: "rpm", done: false, command: "sudo dnf remove hiraku-setup-manager", message: "Falta só tirar o app: isso pede a senha de administrador." } };
      case "quit": return null;
      case "health": return { problems: this.repaired ? 0 : 3, fixable: this.repaired ? 0 : 2,
        general: [{ level: "ok", title: "Último sync", detail: "há 3 min, registry v6 (remote)", fixable: false }],
        modules: [
          { id: "vim", name: "Vim", ok_files: this.repaired ? 41 : 39, items: this.repaired ? [{ level: "ok", title: "Vim completo", detail: "/usr/bin/vim (via /home/icaro/.local/bin/vim)", fixable: false }] : [
            { level: "error", title: "Arquivo antigo no lugar", detail: "Já existia um arquivo seu (de uma instalação antiga?) e o do hiraku nunca foi gravado. Enquanto ele estiver aí, nada do app vale.", path: "/home/icaro/.vimrc", fixable: true },
            { level: "error", title: "Arquivo desatualizado", detail: "As opções mudaram no app, mas a versão nova não chegou a ser gravada (o último sync parou antes ou falhou).", path: "/home/icaro/.vim/hiraku.json", fixable: true },
            { level: "error", title: "O último sync deu erro aqui", detail: "release coc.nvim: sha256 não confere", fixable: false },
            { level: "ok", title: "Vim completo", detail: "/usr/bin/vim (via /home/icaro/.local/bin/vim)", fixable: false }] },
          { id: "tmux", name: "tmux", ok_files: 3, items: [] }] };
      case "sudo_remember": this.sudoSaved = a.on; return a.on ? "Pronto: o dnf instala sem pedir senha." : "O sudo volta a pedir senha para instalar.";
      case "refresh_registry": return null;
      case "open_config_dir": return null;
      case "open_url": this.opened = a.url; return null;
      case "start_sync": {
        if (a.repair) this.repaired = true;
        const mods = a.only ? [a.only] : s.modules;
        let i = 0;
        const tick = () => {
          if (i < mods.length) { this.listeners["sync://line"]?.({ line: `${mods[i]}: aplicando configs`, module: mods[i], kind: "log" }); i++; setTimeout(tick, 250); }
          else { this.report = this.fakeReport(mods); this.listeners["sync://done"]?.({ report: this.report, notes: [], only: a.only }); }
        };
        setTimeout(tick, 200);
        return null;
      }
    }
    throw new Error("mock: comando desconhecido " + cmd);
  },
  fakeReport(mods) {
    mods = mods || this.settings.modules;
    return { started_at: Date.now() / 1000 - 40, finished_at: Date.now() / 1000 - 30, registry_version: 1, registry_source: "embedded", notes: [], modules: mods.map(id => ({ id, status: id === "kitty" ? "updated" : id === "tmux" ? "installed" : "config_only", detail: id === "kitty" ? "0.38.0 -> 0.39.1" : null, major: false, error: null, configs: id === "kitty" ? [{ dest: "/home/icaro/.config/kitty/current-theme.conf", outcome: "Updated" }] : [], hooks: id === "kitty" ? [{ name: "reload-config", outcome: "ran" }] : [] })) };
  },
};

boot();
