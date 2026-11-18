-- Estado persistente (tema, toggles, autocomplete por linguagem...) em JSON.
local M = {}
local file = vim.fn.stdpath("state") .. "/icaro.json"
local cache

local function load()
  if cache then return cache end
  cache = {}
  local f = io.open(file, "r")
  if f then
    local ok, data = pcall(vim.json.decode, f:read("*a"))
    f:close()
    if ok and type(data) == "table" then cache = data end
  end
  return cache
end

-- O que foi escolhido no Hiraku Setup Manager vale por cima do estado local (a
-- Central mostra e altera os mesmos valores do app).
local function hiraku()
  local ok, h = pcall(require, "icaro.hiraku")
  return ok and h or nil
end

function M.get(key, default)
  local h = hiraku()
  local from_app = h and h.from_state(key)
  if from_app ~= nil then return from_app end
  local v = load()[key]
  if v == nil then return default end
  return v
end

function M.set(key, value)
  local h = hiraku()
  if h then h.from_central(key, value) end
  load()[key] = value
  vim.fn.mkdir(vim.fn.fnamemodify(file, ":h"), "p")
  local f = io.open(file, "w")
  if f then
    f:write(vim.json.encode(cache))
    f:close()
  end
end

return M
