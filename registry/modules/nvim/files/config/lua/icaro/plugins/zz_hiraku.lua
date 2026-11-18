-- Plugins desligados no Hiraku Setup Manager (Neovim → Plugins). Este arquivo vem por
-- último na importação: um fragmento com enabled = false desliga o plugin sem mexer
-- nas outras specs. Desligar não apaga nada; o lazy só deixa de carregar.
local hiraku = require("icaro.hiraku")

local repos = {
  neo_tree = "nvim-neo-tree/neo-tree.nvim",
  telescope = "nvim-telescope/telescope.nvim",
  which_key = "folke/which-key.nvim",
  bufferline = "akinsho/bufferline.nvim",
  lualine = "nvim-lualine/lualine.nvim",
  dashboard = "nvimdev/dashboard-nvim",
  gitsigns = "lewis6991/gitsigns.nvim",
  noice = "folke/noice.nvim",
  notify = "rcarriga/nvim-notify",
  indent_blankline = "lukas-reineke/indent-blankline.nvim",
  indent_scope = "echasnovski/mini.indentscope",
  todo_comments = "folke/todo-comments.nvim",
  trouble = "folke/trouble.nvim",
  illuminate = "RRethy/vim-illuminate",
  cursorword = "echasnovski/mini.cursorword",
  scrollbar = "petertriho/nvim-scrollbar",
}

local specs = {}
for key, repo in pairs(repos) do
  if not hiraku.plugin(key, true) then
    specs[#specs + 1] = { repo, enabled = false }
  end
end
-- O fecha-pares não entra aqui: ele liga/desliga ao vivo pela Central (icaro.toggles).
return specs
