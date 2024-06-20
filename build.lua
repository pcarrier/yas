#!/usr/bin/env lua

local operating_systems = {
  mac  = { cmake = "Darwin", nim = "macosx", zig = "macos",
    passL = "-F/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/System/Library/Frameworks",
    curlcflags = "-F/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/System/Library/Frameworks", },
  lin  = { cmake = "Linux", nim = "linux", zig = "linux-musl", },
  win  = { cmake = "Windows", nim = "windows", zig = "windows", bs = ".exe",
    passL = "-lbcrypt -lws2_32", },
}

local architectures = {
  x64 = { cmake = "x86_64", nim = "amd64", zig = "x86_64", },
  a64 = { cmake = "aarch64", nim = "arm64", zig = "aarch64", },
}

local lua_files = {
  "lapi", "lcode", "lctype", "ldebug", "ldo", "ldump", "lfunc", "lgc", "llex", "lmem", "lobject", "lopcodes", "lparser", "lstate", "lstring", "ltable", "ltm", "lundump", "lvm", "lzio",
  "lauxlib", "lbaselib", "lcorolib", "ldblib", "liolib", "lmathlib", "loadlib", "loslib", "lstrlib", "ltablib", "lutf8lib", "linit",
}

local zstd_files = {
  "common/entropy_common", "common/error_private", "common/fse_decompress", "common/pool", "common/threading", "common/xxhash", "common/zstd_common",
  "decompress/huf_decompress", "decompress/zstd_ddict", "decompress/zstd_decompress", "decompress/zstd_decompress_block",
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
  local pwd = os.getenv("PWD")
  local zigcc, zigcpp, zigar, zigranlib = pwd .. "/zigcc", pwd .. "/zigcpp", pwd .. "/zigar", pwd .. "/zigranlib"
  local zig_target = a.zig .. "-" .. o.zig
  exec(
    "zig", "build-lib", "-femit-bin=lib/liblua-" .. zig_target .. ".a",
    "-O", "ReleaseFast", "-fstrip", "-fsingle-threaded", "-lc", "-target", zig_target,
    "-I", "lua/src",
    table.unpack(map(lua_files, function(f) return "lua/src/" .. f .. ".c" end))
  )
  exec(
    "zig", "build-lib", "-femit-bin=lib/libzstd-" .. zig_target .. ".a",
    "-O", "ReleaseFast", "-fstrip", "-fsingle-threaded", "-lc", "-target", zig_target,
    table.unpack(map(zstd_files, function(f) return "zstd/lib/" .. f .. ".c" end))
  )
  exec(
    "zig", "build-lib", "-femit-bin=lib/liblmdb-" .. zig_target .. ".a",
    "-O", "ReleaseFast", "-fstrip", "-fsingle-threaded", "-lc", "-target", zig_target,
    "-I", "lmdb", "lmdb/mdb.c", "lmdb/midl.c"
  )
  exec(
    "zig", "build-lib", "-femit-bin=lib/libtnacl-" .. zig_target .. ".a",
    "-O", "ReleaseFast", "-fstrip", "-fsingle-threaded", "-lc", "-target", zig_target,
    "tweetnacl/tweetnacl.c", "tweetnacl/randombytes.c"
  )
  exec(
    "env", "ZIG_FLAGS=-Wl,--strip-all -target " .. zig_target,
    "cmake", "-GNinja",
    "-DCMAKE_SYSTEM_NAME=" .. o.cmake,
    "-DCMAKE_SYSTEM_PROCESSOR=" .. a.cmake,
    "-DCMAKE_C_COMPILER=" .. zigcc,
    "-DCMAKE_AR=" .. zigar,
    "-DCMAKE_RANLIB=" .. zigranlib,
    --"-DENABLE_ASM=OFF", -- unfortunately, unbreaks the build
    "-DLIBRESSL_SKIP_INSTALL=ON",
    "-DLIBRESSL_APPS=OFF",
    "-DLIBRESSL_TESTS=OFF",
    "-DBUILD_SHARED_LIBS=OFF",
    "-DCMAKE_BUILD_TYPE=MinSizeRel",
    "libressl", "-B", "lib/libressl-" .. zig_target
  )
  exec(
    "env", "ZIG_FLAGS=-Wl,--strip-all -target " .. zig_target,
    "DESTDIR=.", "ninja", "-C", "lib/libressl-" .. zig_target, "install"
  )
  exec(
    "env", "ZIG_FLAGS=-Wl,--strip-all -target " .. zig_target, "CC=" .. zigcc, "CFLAGS=" .. (o.curlcflags or ""),
    "cmake", "-GNinja",
    "-DCMAKE_CROSSCOMPILING=ON",
    "-DCMAKE_SYSTEM_NAME=" .. o.cmake,
    "-DCMAKE_SYSTEM_PROCESSOR=" .. a.cmake,
    "-DCMAKE_C_COMPILER=" .. zigcc,
    "-DCMAKE_AR=" .. zigar,
    "-DCMAKE_RANLIB=" .. zigranlib,
    "-DBUILD_SHARED_LIBS=OFF",
    "-DCURL_USE_OPENSSL=ON",
    "-DOPENSSL_USE_STATIC_LIBS=ON",
    "-DOPENSSL_ROOT_DIR=" .. pwd .. "/lib/libressl-" .. zig_target .. "/usr/local/lib",
    "-DOPENSSL_INCLUDE_DIR=" .. pwd .. "/libressl/include",
    "-DCURL_ZSTD=ON",
    "-DZstd_LIBRARY=" .. pwd .. "/lib/libzstd-" .. zig_target .. ".a",
    "-DZstd_INCLUDE_DIR=" .. pwd .. "/zstd/lib",
    "-DCMAKE_BUILD_TYPE=MinSizeRel",
    "curl", "-B", "lib/curl-" .. zig_target
  )
  exec(
    "env", "ZIG_FLAGS=-Wl,--strip-all -target " .. zig_target, "CC=" .. zigcc,
    "ninja", "-C", "lib/curl-" .. zig_target, "libcurl.a"
  )
  exec(
    "env", "ZIG_FLAGS=-Wl,--strip-all -target " .. zig_target,
    "nim", "compile",
    "--out:bin/yas-" .. o.nim .. "-" .. a.nim .. (o.bs or ""),
    "--os:" .. o.nim,
    "--cpu:" .. a.nim,
    "--clang.exe:" .. zigcc,
    "--clang.linkerexe:" .. zigcc,
    "--passL:" ..
      " -Llib" ..
      " -Llib/curl-" .. zig_target .. "/lib" ..
      " -Llib/libressl-" .. zig_target .. "/crypto" ..
      " -Llib/libressl-" .. zig_target .. "/ssl" ..
      " -llua-" .. zig_target ..
      " -llmdb-" .. zig_target  ..
      " -lzstd-" .. zig_target ..
      " -ltnacl-" .. zig_target ..
      " -lcurl -lcrypto -lssl" ..
      (o.passL and (" " .. o.passL) or ""),
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
