-- Bootstrap do lazy.nvim (gerenciador de plugins).
local path = vim.fn.stdpath("data") .. "/lazy/lazy.nvim"
if not (vim.uv or vim.loop).fs_stat(path) then
  local out = vim.fn.system({ "git", "clone", "--filter=blob:none", "--branch=stable",
    "https://github.com/folke/lazy.nvim.git", path })
  if vim.v.shell_error ~= 0 then
    vim.api.nvim_echo({ { "Falha ao baixar o lazy.nvim (git/internet?):\n" .. out, "ErrorMsg" } }, true, {})
    return
  end
end
vim.opt.rtp:prepend(path)

-- Estilo (app → Neovim → Estilos): cada plugin tem um nível mínimo; abaixo dele o
-- lazy não carrega (continua baixado, então trocar de estilo é instantâneo).
local LEVEL_OF = {
  ["tokyonight.nvim"] = 1, ["catppuccin"] = 1, ["kanagawa.nvim"] = 1, ["nightfox.nvim"] = 1,
  ["nvim-treesitter"] = 1,
  ["nvim-autopairs"] = 2,
  ["blink.cmp"] = 3, ["friendly-snippets"] = 3, ["nvim-lspconfig"] = 3, ["mason.nvim"] = 3,
}
local level = require("icaro.hiraku").level()

require("lazy").setup({
  spec = { { import = "icaro.plugins" } },
  defaults = {
    lazy = true,
    cond = function(plugin)
      return level >= (LEVEL_OF[plugin.name] or 4)
    end,
  },
  install = { colorscheme = { "habamax" } },
  checker = { enabled = false },
  change_detection = { notify = false },
  ui = { border = "rounded" },
  performance = {
    rtp = {
      -- o lazy.nvim reseta o runtimepath; sem isso os temas do Vim somem
      paths = { require("icaro.paths").theme_dir or nil },
      disabled_plugins = { "gzip", "tarPlugin", "tohtml", "tutor", "zipPlugin", "netrwPlugin" } },
  },
})
