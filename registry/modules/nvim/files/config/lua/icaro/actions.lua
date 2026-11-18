-- Ações das teclas de função (F1–F12, Shift+F1–F12...). A tecla de cada uma vem do
-- Hiraku Setup Manager (Neovim → Teclas de função, chave key_<ação> no hiraku.json);
-- sem ela, vale o padrão daqui. A folha de atalhos (F12) mostra as teclas atuais.
local M = {}

local function runner(what) return function() require("icaro.runner").dispatch(what) end end
local function toggles(fn) return function() require("icaro.toggles")[fn]() end end

-- { id, tecla padrão, descrição, comando :X, função }
M.list = {
  { "theme_next", "<F1>", "próximo tema", "Th", function() require("icaro.themes").next() end },
  { "theme_prev", "<S-F1>", "tema anterior", "Pv", function() require("icaro.themes").prev() end },
  { "explorer", "<F2>", "explorer (neo-tree)", "Nt", function() vim.cmd("Neotree toggle") end },
  { "inlay", "<F3>", "inlay hints", "Ih", toggles("inlay") },
  { "hints", "<S-F3>", "dicas de método: assinatura + docs", "Dc", toggles("hints") },
  { "autocomplete", "<F4>", "autocomplete (por linguagem)", "Ac", toggles("autocomplete") },
  { "diagnostics", "<S-F4>", "avisos/erros do código", "Dg", toggles("diagnostics") },
  { "compile", "<F5>", "compilar", "Cp", runner("compile") },
  { "run", "<F6>", "executar", "Rn", runner("run") },
  { "build_run", "<F7>", "compilar + executar", "Br", runner("build_run") },
  { "test", "<F8>", "testar com input.txt", "Ti", runner("test") },
  { "terminal", "<F9>", "terminal", "Te", function() require("icaro.runner").terminal() end },
  { "autopairs", "<F10>", "fecha-pares automático", "Pa", toggles("autopairs") },
  { "matchparen", "<S-F10>", "destaque de parênteses", "Mh", toggles("matchparen") },
  { "quiet", "<S-F11>", "modo silencioso", "Of", toggles("quiet") },
  { "cheatsheet", "<F12>", "esta folha", "Cs", function() require("icaro.cheatsheet").toggle() end },
  { "header", "", "mostra/esconde o header", "Hd", toggles("header") },
  { "central", "<S-F12>", "central de configurações", "RF", function() require("icaro.settings").open() end },
}

-- Teclas atuais: { {tecla, descrição, id}, ... } na ordem da lista.
M.current = {}
local mapped = {}

local function bind(lhs, fn, desc)
  vim.keymap.set("n", lhs, fn, { silent = true, desc = desc })
  vim.keymap.set("i", lhs, function() vim.cmd.stopinsert(); vim.schedule(fn) end, { silent = true, desc = desc })
  mapped[#mapped + 1] = lhs
end

--- (Re)cria os atalhos com as teclas do app. Chamado ao abrir e quando o app muda algo.
function M.apply()
  for _, lhs in ipairs(mapped) do
    pcall(vim.keymap.del, "n", lhs)
    pcall(vim.keymap.del, "i", lhs)
  end
  mapped, M.current = {}, {}
  local hiraku = require("icaro.hiraku")
  local used = {}
  for _, a in ipairs(M.list) do
    local id, default, desc, cmd, fn = unpack(a)
    local key = hiraku.get("key_" .. id, default)
    if type(key) == "string" then key = key:gsub("%s", "") end
    if type(key) == "string" and key ~= "" then
      local ok, err = pcall(bind, key, fn, desc)
      if ok then
        used[key:upper()] = true
        -- Shift+F1..F12 chega como F13..F24 em vários terminais (kitty, GNOME...).
        local n = tonumber(key:match("^<[Ss]%-[Ff](%d+)>$"))
        if n and n <= 12 then
          bind("<F" .. (n + 12) .. ">", fn, desc .. " (compatibilidade de terminal)")
          used[("<F" .. (n + 12) .. ">"):upper()] = true
        end
        M.current[#M.current + 1] = { key, desc .. "  (:" .. cmd .. ")", id }
      else
        vim.notify("Hiraku: tecla " .. key .. " inválida para " .. id .. ": " .. tostring(err), vim.log.levels.WARN)
      end
    end
  end
  -- Shift+F2 e Shift+F5..F9 como alternativas de F2, F5..F9, se estiverem livres.
  for _, n in ipairs({ 2, 5, 6, 7, 8, 9 }) do
    local alt = "<S-F" .. n .. ">"
    if not used[alt:upper()] then
      for _, c in ipairs(M.current) do
        if c[1]:upper() == ("<F" .. n .. ">"):upper() then
          local fn
          for _, a in ipairs(M.list) do if a[1] == c[3] then fn = a[5] end end
          bind(alt, fn, c[2] .. " (alternativo)")
          bind("<F" .. (n + 12) .. ">", fn, c[2] .. " (alternativo)")
        end
      end
    end
  end
end

--- A tecla atual de uma ação (ou nil).
function M.key(id)
  for _, c in ipairs(M.current) do if c[3] == id then return c[1] end end
end

function M.commands()
  for _, a in ipairs(M.list) do
    local fn = a[5]
    vim.api.nvim_create_user_command(a[4], function() fn() end, {})
  end
end

return M
