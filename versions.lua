require 'string'

local function vertable(str)
  local res = {}
  for p in str:gmatch("%d+") do
    table.insert(res, tonumber(p))
  end
  return res
end

local function cmpver(a, b)
  local at, bt = vertable(a), vertable(b)
  for i = 1, math.min(#at, #bt) do
    if at[i] < bt[i] then return -1 end
    if at[i] > bt[i] then return 1 end
  end
  return 0
end

local function maxver(s, extract)
  local v = '0.0.0'
  for m in extract(s) do
    local c = m:match("%d+%.%d+%.%d+")
    if c and cmpver(c, v) > 0 then v = c end
  end
  return v
end

local software = {
  {
    name = 'brotli',
    here = 'deps/brotli/CHANGELOG.md',
    ver = function(c) return maxver(c, function (s) return s:gmatch("## %[[^%]]+%]") end) end,
  },
  {
    name = 'curl',
    here = 'deps/curl/CHANGES',
    ver = function(c) return maxver(c, function (s) return s:gmatch("\nVersion %S+") end) end,
  },
  {
    name = 'libressl',
    here = 'deps/libressl/ChangeLog',
    ver = function(c) return maxver(c, function (s) return s:gmatch("\n[%d%.]+ -") end) end,
  },
  {
    name = 'lmdb',
    here = 'deps/lmdb/CHANGES',
    ver = function(c) return maxver(c, function (s) return s:gmatch("LMDB [%d%.]+ Release") end) end,
  },
  {
    name = 'lua',
    here = 'deps/lua/README',
    ver = function(c) return maxver(c, function (s) return s:gmatch("Lua [%d%.]+") end) end,
  },
  {
    name = 'nghttp2',
    here = 'deps/nghttp2/CMakeLists.txt',
    ver = function(c) return maxver(c, function (s) return s:gmatch("nghttp2 VERSION [^%)]+") end) end,
  },
  {
    name = 'nghttp3',
    here = 'deps/nghttp3/CMakeLists.txt',
    ver = function(c) return maxver(c, function (s) return s:gmatch("nghttp3 VERSION [^%)]+") end) end,
  },
  {
    name = 'ngtcp2',
    here = 'deps/ngtcp2/CMakeLists.txt',
    ver = function(c) return maxver(c, function (s) return s:gmatch("ngtcp2 VERSION [^%)]+") end) end,
  },
  {
    name = 'zlib',
    here = 'deps/zlib/ChangeLog',
    ver = function(c) return maxver(c, function (s) return s:gmatch("Changes in [%d%.]+") end) end,
  },
  {
    name = 'zstd',
    here = 'deps/zstd/CHANGELOG',
    ver = function(c) return maxver(c, function (s) return s:gmatch("V[%d%.]+ %(") end) end,
  },
}

for _, dep in next, software do
  local f = assert(io.open(dep.here, "r"))
  local s = f:read("*a")
  f:close()
  print(dep.name .. "\t" .. dep.ver(s))
end
