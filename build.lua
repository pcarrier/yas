#!/usr/bin/env lua

local operating_systems = {
  mac = { zig = "macos", go = "darwin", p = "lib", s = ".a", gonozig = true },
  lin = { zig = "linux-musl", go = "linux", p = "lib", s = ".a" },
  win = { zig = "windows", go = "windows", p = "", s = ".lib" },
}

local architectures = {
  x64 = { zig = "x86_64", go = "amd64" },
  a64 = { zig = "aarch64", go = "arm64" },
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

local build = function(os, arch)
  local zig_target = arch.zig .. "-" .. os.zig
  local go_target = os.go .. "-" .. arch.go
  local libname = os.p .. "rt-" .. go_target .. os.s
  exec(
    "zig", "build-lib", "-femit-bin=lib/" .. libname,
    "-O", "ReleaseSmall", "-fstrip", "-target", zig_target, "-lc",
    "-I", "lua/src",
    table.unpack(map(lua_files, function(f) return "lua/src/" .. f .. ".c" end
  )))
  exec(
    "env", "GOARCH=" .. arch.go, "GOOS=" .. os.go,  "CGO_ENABLED=1",
    os.gonozig and "CC=clang" or "CC=zig cc -target " .. zig_target,
    "go", "build",
    "-ldflags=-s -w",
    "-o", "bin/yas-" .. go_target,
    "."
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
