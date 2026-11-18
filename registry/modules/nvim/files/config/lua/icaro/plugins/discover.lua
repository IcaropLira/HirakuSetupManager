-- Extras da aba "Descobrir" do Hiraku Setup Manager (Neovim → Descobrir).
-- Todos começam desligados: ligar no app baixa o plugin na próxima vez que o
-- Neovim abrir (o lazy.nvim instala o que estiver faltando). Desligar não apaga
-- nada; o lazy só deixa de carregar.
local hiraku = require("icaro.hiraku")
local on = function(key) return hiraku.plugin(key, false) end

return {
  -- Pular para qualquer lugar visível: <leader>j + 2 letras.
  {
    "folke/flash.nvim",
    enabled = on("flash"),
    opts = { modes = { search = { enabled = false }, char = { enabled = false } } },
    keys = {
      { "<leader>j", mode = { "n", "x", "o" }, function() require("flash").jump() end, desc = "Pular (flash)" },
      { "<leader>J", mode = { "n", "x", "o" }, function() require("flash").treesitter() end, desc = "Selecionar bloco (flash)" },
    },
  },

  -- Trocar o que está em volta: cs"' troca aspas, ds( tira parênteses, ysiw] cerca a palavra.
  { "kylechui/nvim-surround", enabled = on("surround"), event = "VeryLazy", opts = {} },

  -- Editar pastas como texto: "-" abre a pasta do arquivo; renomeie/apague editando linhas e :w.
  {
    "stevearc/oil.nvim",
    enabled = on("oil"),
    cmd = "Oil",
    keys = { { "-", "<cmd>Oil<cr>", desc = "Abrir a pasta (oil)" } },
    opts = { view_options = { show_hidden = true } },
  },

  -- Terminal flutuante: Ctrl+\ abre e fecha.
  {
    "akinsho/toggleterm.nvim",
    enabled = on("toggleterm"),
    keys = { { [[<C-\>]], desc = "Terminal flutuante" } },
    opts = { open_mapping = [[<C-\>]], direction = "float", float_opts = { border = "rounded" } },
  },

  -- Modo foco: <leader>z centraliza o texto e esconde o resto.
  {
    "folke/zen-mode.nvim",
    enabled = on("zen_mode"),
    cmd = "ZenMode",
    keys = { { "<leader>z", "<cmd>ZenMode<cr>", desc = "Modo foco" } },
    opts = { window = { width = 100 } },
  },

  -- Ctrl+h/j/k/l anda entre janelas do Neovim e painéis do tmux sem diferença.
  -- Ligue também o "Navegar com o Neovim/Vim" na aba Descobrir do tmux.
  {
    "christoomey/vim-tmux-navigator",
    enabled = on("tmux_navigator"),
    cmd = { "TmuxNavigateLeft", "TmuxNavigateDown", "TmuxNavigateUp", "TmuxNavigateRight" },
    keys = {
      { "<C-h>", "<cmd>TmuxNavigateLeft<cr>", desc = "Janela/painel à esquerda" },
      { "<C-j>", "<cmd>TmuxNavigateDown<cr>", desc = "Janela/painel abaixo" },
      { "<C-k>", "<cmd>TmuxNavigateUp<cr>", desc = "Janela/painel acima" },
      { "<C-l>", "<cmd>TmuxNavigateRight<cr>", desc = "Janela/painel à direita" },
    },
  },

  -- lazygit dentro do Neovim: <leader>gg (precisa do lazygit; ele está no Descobrir do Shell).
  {
    "kdheepak/lazygit.nvim",
    enabled = on("lazygit"),
    cmd = "LazyGit",
    dependencies = { "nvim-lua/plenary.nvim" },
    keys = { { "<leader>gg", "<cmd>LazyGit<cr>", desc = "lazygit" } },
  },

  -- Sessões por pasta: <leader>ps volta para os arquivos que estavam abertos aqui.
  {
    "folke/persistence.nvim",
    enabled = on("persistence"),
    event = "BufReadPre",
    opts = {},
    keys = {
      { "<leader>ps", function() require("persistence").load() end, desc = "Restaurar sessão desta pasta" },
      { "<leader>pl", function() require("persistence").load({ last = true }) end, desc = "Restaurar a última sessão" },
    },
  },

  -- Markdown com títulos, listas e tabelas desenhados dentro do editor.
  {
    "MeanderingProgrammer/render-markdown.nvim",
    enabled = on("render_markdown"),
    ft = { "markdown" },
    dependencies = { "nvim-treesitter/nvim-treesitter" },
    opts = {},
  },

  -- Pinta #rrggbb, rgb() e nomes de cor com a própria cor.
  {
    "catgoose/nvim-colorizer.lua",
    enabled = on("colorizer"),
    event = "BufReadPre",
    opts = { user_default_options = { names = false } },
  },
}
