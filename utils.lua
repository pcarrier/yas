local M = {}

local function squote(s)
  if not s:find("[\'\"%s]") then return s end
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
  local cmd = table.concat(map({ ... }, squote), " ")
  io.write('$ ', cmd, "\n")
  assert(os.execute(cmd))
end

return M
