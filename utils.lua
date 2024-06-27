local find = require("string").find
local execute = require("os").execute
local tcat = require("table").concat
local write = require("io").write

local M = {}

local function squote(s)
  if not find(s, "[\'\"%s]") then return s end
  return "'" .. s:gsub("'", "'\\''") .. "'"
end

function M.keys(t)
  local r = {}
  for k in next, t do r[#r+1]=k end
  return r
end

local function map(t, f)
  local r = {}
  for i, v in ipairs(t) do r[i] = f(v) end
  return r
end
M.map = map

function M.exec(...)
  local cmd = tcat(map({ ... }, squote), " ")
  write('$ ', cmd, "\n")
  assert(execute(cmd))
end

return M
