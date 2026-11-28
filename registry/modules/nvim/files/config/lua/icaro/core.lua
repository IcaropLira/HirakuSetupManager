-- Liga tudo: temas, toggles, runner, cheatsheet, atalhos e comandos.
local M = {}

function M.setup()
  local themes = require("icaro.themes")
  local toggles = require("icaro.toggles")
  local runner = require("icaro.runner")
  local cheat = require("icaro.cheatsheet")
  local settings = require("icaro.settings")

  -- ---------- temas ----------
  -- depois do lazy: o :colorscheme dos extras dispara o carregamento do plugin
  themes.setup()
  toggles.setup()
  settings.setup()
  local level = require("icaro.hiraku").level()
  -- Dalton: só sintaxe, tema e opções. Competição em diante: templates e teclas.
  if level >= 2 then runner.setup_templates() end

  -- ---------- teclas de função (configuráveis no app) e os comandos :Th :Pv ... ----------
  local actions = require("icaro.actions")
  actions.commands()
  if level >= 2 then actions.apply() end
  vim.api.nvim_create_user_command("LspProgress", function() toggles.lsp_progress() end, {})
  vim.api.nvim_create_user_command("ThemeList", themes.pick, {})

  require("icaro.keymaps")
  require("icaro.autocmds")
  require("icaro.doctor")

  -- Opções do Hiraku Setup Manager: atalhos por cima dos do Ícaro, tema, extras, e
  -- reaplicação automática quando o app grava o hiraku.json.
  local hiraku = require("icaro.hiraku")
  hiraku.apply_keymaps()
  hiraku.setup()
  vim.schedule(function()
    hiraku.apply_theme()
    hiraku.apply_autopairs()
    hiraku.apply_extra()
  end)
end

return M
