-- ============================================================
-- Neovim — Configuração Ícaro Lira
-- Versão Neovim da config do Vim: mesmos atalhos (F1–F12), mesmos
-- aliases (:Th :Pv :Nt ...), mesmos temas, mas com LSP nativo,
-- Treesitter, completion moderno e uma interface mais caprichada.
-- ============================================================
vim.g.mapleader = " "
vim.g.maplocalleader = " "
-- Tecla líder escolhida no Hiraku Setup Manager (hiraku.json), se houver.
pcall(function() require("icaro.hiraku").leader() end)

-- Abertura rápida: cache em disco dos módulos Lua compilados (Neovim 0.9+).
if vim.loader and vim.loader.enable then vim.loader.enable() end

-- Provedores externos (python/node/ruby/perl) que o Neovim procura na abertura
-- e que esta config não usa: desligar economiza dezenas de ms em PCs lentos.
for _, p in ipairs({ "python3", "node", "ruby", "perl" }) do vim.g["loaded_" .. p .. "_provider"] = 0 end

if vim.fn.has("nvim-0.10") == 0 then
  vim.api.nvim_echo({ { "Esta config precisa do Neovim 0.10 ou mais novo. Rode o install.sh da pasta nvim/.", "ErrorMsg" } }, true, {})
  return
end

-- Estilo "Bruto" (app → Neovim → Estilos): o Neovim de fábrica, sem nada.
local ok_h, hiraku = pcall(require, "icaro.hiraku")
if ok_h and hiraku.level() == 0 then
  vim.cmd("syntax off")
  vim.cmd("filetype off")
  return
end

require("icaro.paths")
require("icaro.options")
require("icaro.lazy")      -- instala/carrega os plugins (até o nível do estilo)
require("icaro.core").setup() -- temas, toggles, runner, atalhos, comandos
