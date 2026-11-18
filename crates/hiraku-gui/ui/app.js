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
  const rank = s => ({ choice: 0, number: 1, bool: 2, text: 3 })[s.kind.type];
  return mods.flatMap(m => [...m.options].sort((a, b) => rank(a) - rank(b) || a.key.localeCompare(b.key)));
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
const prettyKey = k => ({ palette: "Paleta de cores", opacity: "Opacidade", blur: "Desfoque", visualizer: "Ao abrir o terminal", fastfetch_color: "Cor do fastfetch", neofetch_color: "Cor do neofetch" })[k] || k.replace(/_/g, " ");
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
      <div class="pj-top"><span class="pj-tile">${esc(GLYPH[m.id] || m.name[0].toUpperCase())}</span><div class="pj-main"><b>${esc(m.name)}</b><small>${m.requires.length ? "usa " + esc(m.requires.join(", ")) : esc(m.id)}</small></div></div>
      <div class="pj-desc">${esc(m.description)}</div>
      <div class="pj-path">${r.installed ? `<b>${esc(r.installed)}</b>` : "não instalado"}${r.pin ? ` · <span class="tag acc">fixado ${esc(r.pin)}</span>` : ""}${st ? ` · <span title="${esc(last.error || last.detail || "")}">${st[0]} ${st[2]}</span>` : ""}</div>
      <div class="pj-act">${opts}<button class="hx-btn ghost sm" data-pin="${m.id}">${r.pin ? "Mudar versão fixada" : "Fixar versão"}</button>${on && r.installed ? `<button class="hx-btn ghost sm" data-apply="${m.id}">Aplicar agora</button>` : ""}</div></article>`;
  }).join("");
  const chip = (k, t) => `<button class="chip ${MOD_FILTER === k ? "on" : ""}" data-filter="${k}">${t}</button>`;
  return `<div class="hx-wrap one"><div class="hx-main">
    <section class="hx-hero sm"><div class="hx-logo">${applogo()}</div><div class="hx-hmain"><h2>Módulos</h2><p class="mut">Ligue o que o hiraku deve instalar e manter em dia. Desligar não desinstala nada.</p></div></section>
    <div class="hx-sec">Seus módulos <span class="n">${SNAP.settings.modules.length}/${mods.length}</span></div>
    <div class="bar">${chip("all", "Todos")}${chip("on", "Habilitados")}${chip("off", "Desligados")}${chip("inst", "Instalados")}</div>
    <div class="pj-grid">${cards || '<div class="hx-panel mut">Nada aqui com esse filtro.</div>'}</div></div></div>`;
}

// ------------------------------------------------------------------ Personalizar (também usado no assistente)
function customHTML(mods, settings, opts = {}) {
  const sections = mods.filter(m => m.options.length).map(m => {
    const specs = sortedSpecs([m]);
    const rows = specs.map(s => optionRow(s, settings)).join("");
    const hasTerm = m.options.some(o => o.key === "palette") && m.options.some(o => o.key === "opacity");
    const off = !opts.wizard && !enabled(m.id) ? `<span class="tag">desabilitado</span>` : "";
    return `<section class="cz-sec glass" id="cz-${m.id}"><header><span class="pj-tile">${esc(GLYPH[m.id] || m.name[0])}</span><h3>${esc(m.name)}</h3>${off}</header>
      ${hasTerm ? `<div class="termwrap"><div class="term" data-term="${m.id}"></div></div>` : ""}${rows}</section>`;
  });
  if (!sections.length) return `<div class="hx-panel mut">Nenhum módulo escolhido tem opções.</div>`;
  if (opts.wizard) return sections.join("");
  const rail = mods.filter(m => m.options.length).map(m => `<button data-jump="${m.id}">${esc(m.name)}<span class="mut">${m.options.length}</span></button>`).join("");
  return `<div class="cz"><div class="cz-rail glass" role="navigation" aria-label="Módulos">${rail}</div><div>${sections.join("")}</div></div>`;
}
function optionRow(spec, settings) {
  const v = optValue(spec, settings), custom = v !== spec.default;
  const reset = custom ? `<button class="reset" data-reset="${spec.module}.${spec.key}">voltar ao padrão</button>` : "";
  const lbl = `<div class="st-lbl"><b>${esc(prettyKey(spec.key))}${reset}</b><small>${esc(spec.description)}</small></div>`;
  const id = `${spec.module}.${spec.key}`;
  switch (spec.kind.type) {
    case "choice": {
      const vals = spec.kind.data;
      if (spec.key === "palette" || vals.length > 6) {
        return `<div class="st-row col">${lbl}<div class="th-grid" data-pal="${id}">${vals.map(x => `<button class="th-card ${x === v ? "on" : ""}" data-set="${id}" data-val="${esc(x)}"><div class="th-prev" data-pv="${esc(x)}"></div><span>${esc(x)}</span></button>`).join("")}</div></div>`;
      }
      return `<div class="st-row">${lbl}<div class="seg">${vals.map(x => `<button class="${x === v ? "on" : ""}" data-set="${id}" data-val="${esc(x)}">${esc(x)}</button>`).join("")}</div></div>`;
    }
    case "number": {
      const { min, max, divisor } = spec.kind.data;
      const unit = divisor === 100 ? "%" : "";
      return `<div class="st-row">${lbl}<div class="rng"><input type="range" min="${min}" max="${max}" value="${esc(v)}" data-num="${id}" data-unit="${unit}"><output>${esc(v)}${unit}</output></div></div>`;
    }
    case "bool":
      return `<div class="st-row">${lbl}<label class="sw"><input type="checkbox" data-bool="${id}" ${v === "true" ? "checked" : ""}><i></i></label></div>`;
    default:
      return `<div class="st-row">${lbl}<input class="txt" data-text="${id}" value="${esc(v)}" spellcheck="false"></div>`;
  }
}
/** Liga os controles de opção. `onChange(spec, valor|null)` grava (hub) ou edita o rascunho (assistente). */
function hydrateCustom(root, settings, onChange) {
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
  // Pré-visualizações das paletas (cores reais dos arquivos do módulo).
  $$("[data-pal]", root).forEach(async grid => {
    const [m, k] = grid.dataset.pal.split(".");
    const map = await previews(m, k);
    $$("[data-pv]", grid).forEach(el => {
      const p = map[el.dataset.pv];
      if (!p) { el.style.background = "var(--bg)"; return; }
      el.style.background = p.background;
      el.innerHTML = `<i style="background:${p.foreground};width:70%"></i><i style="background:${p.color4 || p.foreground};width:45%"></i><div class="dots">${[1, 2, 3, 4, 5, 6].map(n => `<i style="background:${p["color" + n]}"></i>`).join("")}</div>`;
    });
    term(m);
  });
  const set = (id, val) => {
    const spec = specOf(id);
    if (!spec) return;
    const value = val === null || val === spec.default ? null : val;
    onChange(spec, value);
    // Atualiza só o necessário, sem redesenhar a página (mantém o scroll e o foco).
    term(spec.module);
  };
  root.addEventListener("click", e => {
    const b = e.target.closest("[data-set]");
    if (b) {
      b.parentElement.querySelectorAll("[data-set]").forEach(x => x.classList.toggle("on", x === b));
      set(b.dataset.set, b.dataset.val);
      return;
    }
    const r = e.target.closest("[data-reset]");
    if (r) { set(r.dataset.reset, null); rerenderCustom(root, settings, onChange); return; }
    const j = e.target.closest("[data-jump]");
    if (j) { $(`#cz-${j.dataset.jump}`)?.scrollIntoView({ behavior: "smooth", block: "start" }); $$("[data-jump]", root).forEach(x => x.classList.toggle("on", x === j)); }
  });
  $$("[data-num]", root).forEach(inp => {
    const out = inp.nextElementSibling;
    inp.addEventListener("input", () => { out.textContent = inp.value + inp.dataset.unit; set(inp.dataset.num, inp.value); });
  });
  $$("[data-bool]", root).forEach(inp => inp.addEventListener("change", () => set(inp.dataset.bool, String(inp.checked))));
  $$("[data-text]", root).forEach(inp => inp.addEventListener("change", () => set(inp.dataset.text, inp.value)));
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
function viewCustom() {
  const mods = catalog().filter(m => m.supported && m.options.length);
  return `<div class="hx-wrap one"><div class="hx-main">
    <section class="hx-hero sm"><div class="hx-logo">${applogo()}</div><div class="hx-hmain"><h2>Personalizar</h2><p class="mut">Mude e veja na hora. Cada mudança é gravada no settings.toml e aplicada só naquele módulo.</p>
    <div class="hx-cta"><label class="sw"><input type="checkbox" id="live" ${UI.live ? "checked" : ""}><i></i></label><span>Aplicar ao vivo</span></div></div></section>
    <div class="hx-sec">Opções</div>${customHTML(mods, SNAP.settings)}</div></div>`;
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
    const t = e.target.closest("[data-v],[data-act],[data-goto],[data-filter],[data-pin],[data-apply],[data-cf],[data-resolve],[data-every]");
    if (!t || !v.contains(t)) return;
    const d = t.dataset;
    if (d.v) return go(d.v);
    if (d.goto) { VIEW = "custom"; render(); setTimeout(() => $(`#cz-${d.goto}`)?.scrollIntoView({ block: "start" }), 30); return; }
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
  };
  v.onchange = async e => {
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
  SYNC = { only, lines: [], progress: null, quiet: !!opts.quiet };
  dock();
  try { await call("start_sync", { only }); }
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
  d.innerHTML = `<header><span class="hx-dot">⟳</span><b>${SYNC.only ? `Aplicando ${esc(SYNC.only)}…` : "Sincronizando…"}</b><button class="x" id="dock-x" title="Esconder">–</button></header>
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
    if (was && was.only && !f) toast(`${was.only}: aplicado ✓`);
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
function sudoDialog(mods) {
  return new Promise(resolve => {
    const p = modal(`<div class="st-head"><h2>Senha de administrador</h2><button class="x" data-x>✕</button></div>
      <div class="st-pad"><p class="mut" style="margin-top:0">Para instalar com o gerenciador de pacotes do sistema: <b>${esc(mods.join(", "))}</b>.</p>
      <input class="field" type="password" id="pw" placeholder="sua senha do sudo" autocomplete="current-password">
      <div class="err-t" id="pwerr"></div><small class="mut">A senha vai direto para o <code>sudo</code> e não é guardada. Prefere sem sudo? Mude o modo para "Só no meu usuário" nas Configurações.</small>
      <div class="hx-cta" style="justify-content:flex-end"><button class="hx-btn ghost" data-x>Cancelar</button><button class="hx-btn" id="pwok">Continuar</button></div></div>`, "small");
    const done = v => { closeModal(); resolve(v); };
    $$("[data-x]", p).forEach(b => b.onclick = () => done(false));
    const go = async () => {
      $("#pwok").disabled = true;
      try { await call("sudo_auth", { password: $("#pw").value }); done(true); }
      catch (e) { $("#pwerr").textContent = String(e); $("#pwok").disabled = false; $("#pw").select(); }
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
      <div class="st-sec">Este app</div>
      <div class="st-row"><div class="st-lbl"><b>Atualizar o app sozinho</b><small>Procura versão nova ao abrir e a cada 6 h, baixa em segundo plano e só pede para reiniciar.</small></div><label class="sw"><input type="checkbox" id="autoapp" ${UI.autoApp !== false ? "checked" : ""}><i></i></label></div>
      <div class="st-row"><div class="st-lbl"><b>Arquivo de configurações</b><small style="font-family:var(--mono)">${esc(INFO.settings_file)}</small></div><button class="hx-btn ghost sm" id="od">Abrir pasta</button></div>
      <div class="st-row"><div class="st-lbl"><b>Refazer a instalação</b><small>Abre o assistente com as escolhas atuais.</small></div><button class="hx-btn ghost sm" id="rw">Abrir assistente</button></div>`;
    body.onclick = async e => {
      const t = e.target.closest("[data-mode]");
      if (t) { try { await call("set_mode", { mode: t.dataset.mode || null }); toast("Modo salvo. Vale no próximo sync."); } catch (err) { fail(err); } await reload(); settingsOpen(); }
    };
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
let APP_UPDATE = null; // {update, version, ready}
async function checkAppUpdate(manual) {
  const U = T && T.updater;
  if (!U) { if (manual) toast("A atualização do app só funciona no app instalado."); return; }
  if (APP_UPDATE && APP_UPDATE.ready) return appUpdateReady();
  let up;
  try { up = await U.check(); }
  catch (e) { if (manual) fail("Não consegui verificar: " + e); return; }
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
  a.textContent = `Reiniciar para atualizar (v${APP_UPDATE.version})`;
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
  file: [["Sincronizar tudo", "Ctrl+S", () => runSync(null)], ["Buscar registry novo", "", () => action("registry")], "-", ["Abrir pasta de configurações", "", () => action("opendir")], "-", ["Sair", "Ctrl+Q", () => T && T.process ? T.process.exit(0) : window.close()]],
  edit: [["Configurações…", "Ctrl+,", () => settingsOpen()]],
  view: [["Início", "Ctrl+1", () => go("home")], ["Módulos", "Ctrl+2", () => go("modules")], ["Personalizar", "Ctrl+3", () => go("custom")], ["Conflitos", "Ctrl+4", () => go("conflicts")], ["Atualizações", "Ctrl+5", () => go("updates")], "-", ["Alternar claro/escuro", "Ctrl+Shift+L", toggleDark]],
  go: [["Refazer instalação…", "", () => wzOpen(false)], ["Arquivo settings.toml", "", () => action("opendir")]],
  help: [["Atalhos de teclado", "", () => settingsOpen("keys")], ["Verificar atualizações", "", () => checkAppUpdate(true)], "-", ["Sobre o Hiraku Setup", "", () => settingsOpen("about")]],
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
  const map = { s: () => runSync(null), ",": () => settingsOpen(), 1: () => go("home"), 2: () => go("modules"), 3: () => go("custom"), 4: () => go("conflicts"), 5: () => go("updates") };
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
    return [
      m("bat", "bat", "Substituto do cat com realce de sintaxe"),
      m("font-jetbrains-mono", "JetBrainsMono Nerd Font", "Fonte com ícones (Nerd Font) usada pelo kitty, vim e nvim"),
      m("icaro-themes", "Temas do Ícaro", "Colorschemes compartilhados pelo vim e pelo nvim (90 arquivos)"),
      m("kitty", "kitty", "Terminal com aceleração de GPU, com paleta e transparência configuráveis", ["font-jetbrains-mono"], [o("palette", { type: "choice", data: Object.keys(this.pal) }, "navy", "Paleta de cores (arquivos em files/palettes)"), o("opacity", { type: "number", data: { min: 30, max: 100, divisor: 100 } }, "80", "Opacidade do fundo, em % (100 = sólido)"), o("blur", { type: "number", data: { min: 0, max: 64, divisor: 1 } }, "18", "Desfoque do fundo (0 = desligado)")]),
      m("nvim", "Neovim", "Neovim com lazy.nvim, LSP, temas do Ícaro e autocomplete", ["icaro-themes", "font-jetbrains-mono"]),
      m("shell", "Shell (bash/zsh)", "Bloco no .bashrc/.zshrc, aliases, starship e o visualizer de boas-vindas", [], [{ module: "shell", key: "visualizer", kind: { type: "choice", data: ["none", "fastfetch", "neofetch"] }, default: "none", description: "Programa exibido ao abrir um shell" }, { module: "shell", key: "fastfetch_color", kind: { type: "text" }, default: "@33", description: "Cor do fastfetch (código do --color)" }]),
      m("starship", "starship", "Prompt rápido e configurável"),
      m("tmux", "tmux", "Multiplexador de terminal (prefixo Ctrl+A)"),
      m("vim", "Vim", "Vim com tema, plugins (airline, nerdtree, coc...) e wrapper", ["icaro-themes", "font-jetbrains-mono"]),
    ];
  },
  report: null,
  async call(cmd, a) {
    await new Promise(r => setTimeout(r, 30));
    if (!this.settings) this.settings = { schema: 1, modules: [], pins: {}, options: {} };
    const s = this.settings;
    switch (cmd) {
      case "info": return { version: "0.1.0", env_label: "linux · fedora · x86_64 · sudo disponível", os: "linux", can_root: true, is_root: false, first_run: this.first && !new URLSearchParams(location.search).has("hub"), registry_version: 1, registry_source: "embedded", settings_file: "/home/icaro/.config/hiraku/settings.toml", state_dir: "/home/icaro/.local/state/hiraku", running: false };
      case "snapshot": {
        if (new URLSearchParams(location.search).has("hub") && !s.modules.length) { s.modules = ["kitty", "font-jetbrains-mono", "shell", "nvim", "tmux"]; s.options = { kitty: { palette: "nord" } }; this.first = false; this.report = this.fakeReport(); }
        const cat = this.cat();
        return {
          catalog: cat, settings: JSON.parse(JSON.stringify(s)), scheduled: this.sched || false, report: this.report,
          conflicts: s.modules.includes("shell") ? [{ path: "/home/icaro/.bashrc", module: "shell" }] : [],
          modules: cat.map(m => ({ id: m.id, description: m.description, enabled: s.modules.includes(m.id), installed: this.report && s.modules.includes(m.id) ? ({ kitty: "0.39.1", tmux: "3.5a", nvim: "0.11.2" }[m.id] || "configs") : null, pin: s.pins[m.id] || null, supported: true })),
        };
      }
      case "previews": return Object.fromEntries(Object.entries(this.pal).map(([k, [bg, fg, c]]) => [k, Object.assign({ background: bg, foreground: fg }, ...c.map((x, i) => ({ ["color" + i]: x })))]));
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
      case "refresh_registry": return null;
      case "open_config_dir": return null;
      case "start_sync": {
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
