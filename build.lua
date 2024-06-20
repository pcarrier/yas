#!/usr/bin/env lua

local operating_systems = {
  mac = { nim = "macosx",  zig = "macos",      p = "lib", s = ".a", passL = "-F/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/System/Library/Frameworks" },
  lin = { nim = "linux",   zig = "linux-musl", p = "lib", s = ".a", },
  win = { nim = "windows", zig = "windows",               s = ".lib", },
}

local architectures = {
  x64 = { nim = "amd64", zig = "x86_64", },
  a64 = { nim = "arm64", zig = "aarch64", },
}

local lua_files = {
  'lapi', 'lcode', 'lctype', 'ldebug', 'ldo', 'ldump', 'lfunc', 'lgc', 'llex', 'lmem', 'lobject', 'lopcodes', 'lparser', 'lstate', 'lstring', 'ltable', 'ltm', 'lundump', 'lvm', 'lzio',
  'lauxlib', 'lbaselib', 'lcorolib', 'ldblib', 'liolib', 'lmathlib', 'loadlib', 'loslib', 'lstrlib', 'ltablib', 'lutf8lib', 'linit',
}

local zstd_files = {
  'common/entropy_common', 'common/error_private', 'common/fse_decompress', 'common/pool', 'common/threading', 'common/xxhash', 'common/zstd_common',
  'decompress/huf_decompress', 'decompress/zstd_ddict', 'decompress/zstd_decompress', 'decompress/zstd_decompress_block',
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
    "zig", "build-lib", "-femit-bin=lib/" .. (o.p or "") .. "lua-" .. zig_target .. o.s,
    "-O", "ReleaseSmall", "-fstrip", "-fsingle-threaded", "-lc", "-target", zig_target,
    "-I", "lua/src",
    table.unpack(map(lua_files, function(f) return "lua/src/" .. f .. ".c" end))
  )
  exec(
    "zig", "build-lib", "-femit-bin=lib/" .. (o.p or "") .. "zstd-" .. zig_target .. o.s,
    "-O", "ReleaseSmall", "-fstrip", "-fsingle-threaded", "-lc", "-target", zig_target,
    table.unpack(map(zstd_files, function(f) return "zstd/lib/" .. f .. ".c" end))
  )
  exec(
    "zig", "build-lib", "-femit-bin=lib/" .. (o.p or "") .. "lmdb-" .. zig_target .. o.s,
    "-O", "ReleaseSmall", "-fstrip", "-fsingle-threaded", "-lc", "-target", zig_target,
    "-I", "lmdb", "lmdb/mdb.c", "lmdb/midl.c"
  )
  exec(
    "zig", "build-lib", "-femit-bin=lib/" .. (o.p or "") .. "tnacl-" .. zig_target .. o.s,
    "-O", "ReleaseSmall", "-fstrip", "-fsingle-threaded", "-lc", "-target", zig_target,
    "tweetnacl/tweetnacl.c", "tweetnacl/randombytes.c"
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
    "--passC:-Ilua/src -Ilmdb -Izstd/lib -Itweetnacl",
    "--passL:-Llib -llua-" .. zig_target ..
      " -llmdb-" .. zig_target  ..
      " -lzstd-" .. zig_target ..
      " -ltnacl-" .. zig_target
      .. (o.passL and (" " .. o.passL) or ""),
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
