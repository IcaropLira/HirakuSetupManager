-- Opções escolhidas no Hiraku Setup Manager (~/.config/nvim/hiraku.json).
--
-- - Aplicadas ao abrir (opções, atalhos, tema, plugins) e reaplicadas sozinhas
--   quando o app grava o arquivo de novo: dá para mudar tudo com o Neovim aberto.
-- - A Central de configurações (:RF / Shift+F12) lê daqui e, ao mudar algo,
--   avisa o app (`hiraku set nvim chave=valor`): os dois nunca discordam.
local M = { data = {}, applied = {} }
M.path = vim.fn.stdpath("config") .. "/hiraku.json"

local uv = vim.uv or vim.loop

function M.load()
  local f = io.open(M.path, "r")
  if not f then
    M.data = {}
    return false
  end
  local ok, data = pcall(vim.json.decode, f:read("*a"))
  f:close()
  M.data = (ok and type(data) == "table") and data or {}
  return ok
end

function M.has(key)
  local v = M.data[key]
  return v ~= nil and v ~= vim.NIL
end

function M.get(key, default)
  if M.has(key) then return M.data[key] end
  return default
end

function M.plugin(name, default)
  return M.get("plugin_" .. name, default ~= false)
end

M.load()

-- ---------------------------------------------------------------- Central <-> app
-- Chaves do icaro/state.lua (Central) que correspondem a opções do app.
-- Toggles da Central (icaro/toggles.lua) -> opção do app.
local TOGGLE_KEYS = { inlay = "inlay", matchparen = "matchparen", header = "header", hints = "hints",
  diag = "diagnostics", lsp_progress = "lsp_progress" }
local AC_LANGS = { "cpp", "java", "python", "outros" }

local function hub_key(state_key)
  if state_key == "theme" then return "colorscheme" end
  if state_key == "pairs" then return "autopairs" end
  if state_key == "nerd_font" then return "nerd_font" end
  if TOGGLE_KEYS[state_key] then return TOGGLE_KEYS[state_key] end
  local opt = state_key:match("^option_(.+)$")
  if opt then return opt end
  if state_key:match("^plugin_") then return state_key end
end

--- Valor do app para uma chave do state.lua, ou nil se o app não define.
function M.from_state(state_key)
  if state_key == "ac" then
    -- autocomplete por linguagem: { cpp = bool, ... } montado das autocomplete_<lang>
    local t, any = {}, false
    for _, l in ipairs(AC_LANGS) do
      if M.has("autocomplete_" .. l) then t[l] = M.data["autocomplete_" .. l]; any = true end
    end
    return any and t or nil
  end
  local k = hub_key(state_key)
  if not k or not M.has(k) then return nil end
  local v = M.data[k]
  if k == "colorscheme" and v == "auto" then return nil end
  if k == "colorcolumn" and v == "0" then return "" end
  return v
end

local function cli()
  for _, exe in ipairs({ "hiraku", "hiraku-setup-manager" }) do
    if vim.fn.executable(exe) == 1 then return exe end
  end
end

-- Gravações no app, uma de cada vez (duas ao mesmo tempo disputariam o settings.toml).
local queue, busy = {}, false

local function pump()
  if busy or #queue == 0 then return end
  local exe = cli()
  if not exe then
    queue = {}
    return
  end
  busy = true
  local key, text = unpack(table.remove(queue, 1))
  local ok = pcall(vim.system, { exe, "set", "nvim", key .. "=" .. text }, { text = true }, function(r)
    vim.schedule(function()
      busy = false
      if r.code ~= 0 then
        vim.notify("Hiraku: não consegui salvar " .. key .. " no app: " .. (r.stderr or ""), vim.log.levels.WARN)
      end
      pump()
    end)
  end)
  if not ok then busy = false end
end

--- A Central mudou algo: grava no app também (em segundo plano).
function M.from_central(state_key, value)
  if state_key == "ac" and type(value) == "table" then
    for _, l in ipairs(AC_LANGS) do
      if value[l] ~= nil then M.from_central("ac_" .. l, value[l] ~= false) end
    end
    return
  end
  local k = state_key:match("^ac_") and ("autocomplete_" .. state_key:sub(4)) or hub_key(state_key)
  if not k or not M.has(k) then return end
  if k == "colorcolumn" and value == "" then value = "0" end
  if M.data[k] == value then return end
  M.data[k] = value
  local text = type(value) == "boolean" and (value and "true" or "false") or tostring(value)
  -- Se a mesma chave já está na fila, só o último valor importa.
  for i = #queue, 1, -1 do
    if queue[i][1] == k then table.remove(queue, i) end
  end
  table.insert(queue, { k, text })
  pump()
end

-- ---------------------------------------------------------------- aplicar
local LEADER = { space = " ", comma = ",", backslash = "\\" }

--- Antes de qualquer atalho ser criado (init.lua).
function M.leader()
  local l = LEADER[M.get("leader", "space")] or " "
  vim.g.mapleader = l
  vim.g.maplocalleader = l
end

local BOOLS = { "number", "relativenumber", "cursorline", "cursorcolumn", "list", "wrap", "linebreak",
  "spell", "showmode", "smartindent", "autoindent", "backup", "writebackup",
  "expandtab", "undofile", "swapfile", "confirm", "splitright", "splitbelow", "ignorecase", "smartcase",
  "hlsearch", "incsearch" }

function M.apply_options()
  local o, g = vim.opt, M.get
  for _, k in ipairs(BOOLS) do
    if type(g(k)) == "boolean" then o[k] = g(k) end
  end
  if M.has("colorcolumn") then o.colorcolumn = g("colorcolumn") == "0" and "" or g("colorcolumn") end
  if M.has("signcolumn") then o.signcolumn = g("signcolumn") end
  if M.has("scrolloff") then o.scrolloff = g("scrolloff") end
  if M.has("tabsize") then
    local n = g("tabsize")
    o.tabstop, o.shiftwidth, o.softtabstop = n, n, n
  end
  if M.has("mouse") then o.mouse = g("mouse") == "off" and "" or g("mouse") end
  if M.has("timeoutlen") then o.timeoutlen = g("timeoutlen") end
  if M.has("clipboard") then
    -- Depois da 1ª tela (a detecção do clipboard é lenta em alguns PCs).
    vim.schedule(function() vim.opt.clipboard = g("clipboard") == "system" and "unnamedplus" or "" end)
  end
end

-- Atalhos criados por aqui; ao reaplicar, saem e o atalho anterior (do Ícaro) volta.
local function map(mode, lhs, rhs, opts)
  local prev = vim.fn.maparg(lhs, mode, false, true)
  table.insert(M.applied, { mode = mode, lhs = lhs, prev = (type(prev) == "table" and next(prev)) and prev or nil })
  vim.keymap.set(mode, lhs, rhs, vim.tbl_extend("force", { silent = true }, opts or {}))
end

local function unmap_all()
  for i = #M.applied, 1, -1 do
    local a = M.applied[i]
    pcall(vim.keymap.del, a.mode, a.lhs)
    if a.prev then pcall(vim.fn.mapset, a.mode, false, a.prev) end
  end
  M.applied = {}
end

function M.apply_keymaps()
  unmap_all()
  -- teclas de função (key_<ação>): antes dos "Meus atalhos", que valem por cima
  local ok, actions = pcall(require, "icaro.actions")
  if ok and next(actions.current) ~= nil then actions.apply() end
  local nx = { "n", "x" }
  local mode = M.get("delete_mode", "copy")
  if mode == "blackhole" then
    -- d, c, x, s jogam fora sem tocar no que foi copiado; <leader>d recorta.
    for _, k in ipairs({ "d", "D", "c", "C", "x", "X", "s", "S" }) do
      for _, m in ipairs(nx) do map(m, k, '"_' .. k, { desc = "Apagar sem copiar" }) end
    end
    for _, m in ipairs(nx) do map(m, "<leader>d", "d", { desc = "Recortar (apaga e copia)" }) end
    map("n", "<leader>dd", "dd", { desc = "Recortar a linha" })
    map("n", "<leader>D", "D", { desc = "Recortar até o fim da linha" })
  elseif mode == "x_only" then
    for _, k in ipairs({ "x", "X", "c", "C", "s", "S" }) do
      for _, m in ipairs(nx) do map(m, k, '"_' .. k, { desc = "Apagar sem copiar" }) end
    end
  end
  if M.get("paste_keeps", true) then
    -- No visual, P troca a seleção sem copiar o texto substituído.
    map("x", "p", "P", { desc = "Colar sem perder o que foi copiado" })
  end
  local list = M.get("keymaps", {})
  if type(list) == "table" then
    for _, km in ipairs(list) do
      if type(km) == "table" and km.lhs and km.rhs and km.mode then
        for m in tostring(km.mode):gmatch(".") do
          local ok, err = pcall(map, m, km.lhs, km.rhs, { desc = km.desc, remap = km.remap == true })
          if not ok then vim.notify("Hiraku: atalho " .. km.lhs .. " inválido: " .. tostring(err), vim.log.levels.WARN) end
        end
      end
    end
  end
end

local function transparent()
  for _, group in ipairs({ "Normal", "NormalNC", "NormalFloat", "SignColumn", "EndOfBuffer", "LineNr",
    "FoldColumn", "StatusLine", "NeoTreeNormal", "NeoTreeNormalNC" }) do
    local hl = vim.api.nvim_get_hl(0, { name = group, link = false })
    hl.bg, hl.ctermbg = nil, nil
    pcall(vim.api.nvim_set_hl, 0, group, hl)
  end
end

function M.apply_theme()
  local name = M.get("colorscheme", "auto")
  if name == "rose-pine" then name = "o_do_lol" end -- o antigo "o do LOL"
  if name ~= "auto" and vim.g.colors_name ~= name then
    local ok, err = pcall(vim.cmd.colorscheme, name)
    if not ok then vim.notify("Hiraku: tema '" .. name .. "' não encontrado: " .. tostring(err), vim.log.levels.WARN) end
  end
  if M.get("transparent", false) then transparent() end
end

function M.apply_autopairs()
  if not M.has("autopairs") then return end
  local ok, toggles = pcall(require, "icaro.toggles")
  if ok and toggles.s and toggles.s.pairs ~= M.get("autopairs") then toggles.autopairs() end
end

--- Central: com o editor aberto, aciona o toggle quando o app discorda.
function M.apply_toggles()
  local ok, t = pcall(require, "icaro.toggles")
  if not ok or not t.s then return end
  local function sync(key, current, fn)
    if M.has(key) and (current and true or false) ~= (M.data[key] and true or false) then pcall(t[fn]) end
  end
  sync("inlay", t.s.inlay, "inlay")
  sync("matchparen", t.s.matchparen, "matchparen")
  sync("header", t.s.header, "header")
  sync("hints", t.s.hints, "hints")
  sync("diagnostics", t.s.diag, "diagnostics")
  sync("lsp_progress", vim.g.icaro_lsp_progress, "lsp_progress")
  for _, l in ipairs(AC_LANGS) do
    local k = "autocomplete_" .. l
    if M.has(k) and t.set_ac then t.set_ac(l, M.data[k]) end
  end
end

--- Template de arquivo novo do app (template_<lang>), ou nil se o app não define.
function M.template(key)
  local k = "template_" .. key
  if not M.has(k) then return nil end
  local text = M.data[k]
  if type(text) ~= "string" then return nil end
  local lines = vim.split(text, "\n", { plain = true })
  if #lines > 1 and lines[#lines] == "" then table.remove(lines) end
  return lines
end

function M.apply_extra()
  local code = M.get("extra", "")
  if type(code) ~= "string" or code:match("^%s*$") then return end
  local fn, err = load(code, "hiraku: configuração extra")
  if not fn then
    vim.notify("Hiraku: erro na configuração extra: " .. err, vim.log.levels.ERROR)
    return
  end
  local ok, run_err = pcall(fn)
  if not ok then vim.notify("Hiraku: erro na configuração extra: " .. tostring(run_err), vim.log.levels.ERROR) end
end

function M.apply_all()
  M.apply_options()
  M.apply_keymaps()
  M.apply_theme()
  M.apply_autopairs()
  M.apply_toggles()
  M.apply_extra()
end

-- ---------------------------------------------------------------- ao vivo
function M.setup()
  local group = vim.api.nvim_create_augroup("hiraku", { clear = true })
  vim.api.nvim_create_autocmd("TextYankPost", {
    group = group,
    callback = function()
      if M.get("yank_highlight", true) then (vim.hl or vim.highlight).on_yank({ timeout = 180 }) end
    end,
  })
  vim.api.nvim_create_autocmd("ColorScheme", {
    group = group,
    callback = function() if M.get("transparent", false) then transparent() end end,
  })
  vim.api.nvim_create_user_command("HirakuReload", function() M.reload(true) end,
    { desc = "Reaplicar as opções do Hiraku Setup Manager" })

  -- O app grava com troca atômica (arquivo novo + rename): observa a pasta.
  if uv and uv.new_fs_event then
    local watcher = uv.new_fs_event()
    local timer = uv.new_timer()
    local dir = vim.fn.fnamemodify(M.path, ":h")
    watcher:start(dir, {}, function(err, fname)
      if err or (fname and fname ~= "hiraku.json") then return end
      timer:stop()
      timer:start(200, 0, vim.schedule_wrap(function() M.reload(false) end))
    end)
    M.watcher = watcher
  end
end

function M.reload(verbose)
  local before = vim.json.encode(M.data)
  if not M.load() then return end
  if not verbose and vim.json.encode(M.data) == before then return end
  M.apply_all()
  vim.notify("Configurações do Hiraku aplicadas", vim.log.levels.INFO, { title = "Hiraku" })
end

return M
