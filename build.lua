#!/usr/bin/env lua

local operating_systems = {
  mac = { nim = "macosx", zig = "macos", p = "lib", s = ".a", },
  lin = { nim = "linux", zig = "linux-musl", p = "lib", s = ".a", },
  win = { nim = "windows", zig = "windows", p = "", s = ".lib", },
}

local architectures = {
  x64 = { nim = "amd64", zig = "x86_64", },
  a64 = { nim = "arm64", zig = "aarch64", },
}

local lua_files = {
  'lapi', 'lcode', 'lctype', 'ldebug', 'ldo', 'ldump', 'lfunc', 'lgc', 'llex', 'lmem', 'lobject', 'lopcodes', 'lparser', 'lstate', 'lstring', 'ltable', 'ltm', 'lundump', 'lvm', 'lzio',
  'lauxlib', 'lbaselib', 'lcorolib', 'ldblib', 'liolib', 'lmathlib', 'loadlib', 'loslib', 'lstrlib', 'ltablib', 'lutf8lib', 'linit',
}

local squote = function(s)
  if not s:find("[\'\"%s]") then return s end
  return "'" .. s:gsub("'", "'\\''") .. "'"
end

local map = function(t, f)
  local r = {}
  for i, v in ipairs(t) do r[i] = f(v) end
  return r
end

local exec = function(...)
  local cmd = table.concat(map({...}, squote), " ")
  print('$ ' .. cmd)
  assert(os.execute(cmd))
end

local build = function(o, a)
  local zigcc = os.getenv("PWD") .. "/zigcc"
  local zig_target = a.zig .. "-" .. o.zig
  exec(
    "zig", "build-lib", "-femit-bin=lib/" .. o.p .. "lua-" .. zig_target .. o.s,
    "-O", "ReleaseSmall", "-fstrip", "-fsingle-threaded", "-lc", "-target", zig_target,
    "-I", "lua/src",
    table.unpack(map(lua_files, function(f) return "lua/src/" .. f .. ".c" end))
  )
  exec(
    "zig", "build-lib", "-femit-bin=lib/" .. o.p .. "lmdb-" .. zig_target .. o.s,
    "-O", "ReleaseSmall", "-fstrip", "-fsingle-threaded", "-lc", "-target", zig_target,
    "-I", "lmdb", "lmdb/mdb.c"
  )
  exec(
    "env", "ZIG_FLAGS=-target " .. zig_target,
    "nim", "compile",
    "--out:bin/yas-" .. o.nim .. "-" .. a.nim,
    "--cc:clang",
    "--os:" .. o.nim,
    "--cpu:" .. a.nim,
    "--clang.exe:" .. zigcc,
    "--clang.linkerexe:" .. zigcc,
    "--passC:-Ilua/src -Ilmdb",
    "--passL:-Llib -llua-" .. zig_target .. " -llmdb-" .. zig_target,
    "src/yas.nim"
  )
end

if #arg > 0 then
  for _, os_arch in ipairs(arg) do
    local os, arch = os_arch:match("([^-]+)-([^-]+)")
    if not os or not arch then
      error("Invalid os-arch argument: " .. os_arch)
    end
    build(operating_systems[os], architectures[arch])
  end
else
  for _, os in pairs(operating_systems) do
    for _, arch in pairs(architectures) do
      build(os, arch)
    end
  end  
end
