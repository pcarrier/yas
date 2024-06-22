#!/usr/bin/env lua

local operating_systems = {
    mac = {
        cmake = "Darwin",
        zig = "macos",
        curlcflags = "-F/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/System/Library/Frameworks",
        abi = "",
    },
    lin = {
        cmake = "Linux",
        zig = "linux-musl",
        abi = "ELF",
    },
    win = {
        cmake = "Windows",
        zig = "windows",
        bs = ".exe",
        abi = "",
    },
}

local architectures = {
    x64 = { zig = "x86_64", },
    a64 = { zig = "aarch64", },
}

local lua_files = {
    "lapi", "lcode", "lctype", "ldebug", "ldo", "ldump", "lfunc", "lgc", "llex", "lmem", "lobject", "lopcodes", "lparser",
    "lstate", "lstring", "ltable", "ltm", "lundump", "lvm", "lzio",
    "lauxlib", "lbaselib", "lcorolib", "ldblib", "liolib", "lmathlib", "loadlib", "loslib", "lstrlib", "ltablib",
    "lutf8lib", "linit",
}

local zlib_files = {
    "adler32", "compress", "crc32", "deflate", "gzclose", "gzlib", "gzread", "gzwrite", "inflate", "infback", "inftrees",
    "inffast", "trees", "uncompr", "zutil", "contrib/minizip/unzip",
}

local zstd_files = {
    "common/entropy_common", "common/error_private", "common/fse_decompress", "common/pool", "common/threading",
    "common/xxhash", "common/zstd_common",
    "decompress/huf_decompress", "decompress/zstd_ddict", "decompress/zstd_decompress",
    "decompress/zstd_decompress_block",
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
    local cmd = table.concat(map({ ... }, squote), " ")
    print('$ ' .. cmd)
    assert(os.execute(cmd))
end

local build = function(o, a)
    local pwd = os.getenv("PWD")
    local zigcc, zigcpp, zigar, zigranlib = pwd .. "/zigcc", pwd .. "/zigcpp", pwd .. "/zigar", pwd .. "/zigranlib"
    local zig_target = a.zig .. "-" .. o.zig
    exec(
        "zig", "build-lib", "-femit-bin=lib/liblua-" .. zig_target .. ".a",
        "-OReleaseFast", "-fstrip", "-fsingle-threaded", "-lc", "-target", zig_target,
        "-Ilua/src",
        table.unpack(map(lua_files, function(f) return "lua/src/" .. f .. ".c" end))
    )
    exec(
        "zig", "build-lib", "-femit-bin=lib/libz-" .. zig_target .. ".a",
        "-OReleaseFast", "-fstrip", "-fsingle-threaded", "-lc", "-target", zig_target,
        "-Izlib", "-DHAVE_SYS_TYPES_H", "-DHAVE_STDINT_H", "-DHAVE_STDDEF_H", "-DOFF64_T", "-D_LARGEFILE64_SOURCE=1",
        "-DZ_HAVE_UNISTD_H",
        table.unpack(map(zlib_files, function(f) return "zlib/" .. f .. ".c" end))
    )
    exec(
        "zig", "build-lib", "-femit-bin=lib/libzstd-" .. zig_target .. ".a",
        "-OReleaseFast", "-fstrip", "-fsingle-threaded", "-lc", "-target", zig_target,
        table.unpack(map(zstd_files, function(f) return "zstd/lib/" .. f .. ".c" end))
    )
    exec(
        "zig", "build-lib", "-femit-bin=lib/liblmdb-" .. zig_target .. ".a",
        "-OReleaseFast", "-fstrip", "-fsingle-threaded", "-lc", "-target", zig_target,
        "-I", "lmdb", "lmdb/mdb.c", "lmdb/midl.c"
    )
    exec(
        "zig", "build-lib", "-femit-bin=lib/libtnacl-" .. zig_target .. ".a",
        "-OReleaseFast", "-fstrip", "-fsingle-threaded", "-lc", "-target", zig_target,
        "tweetnacl/tweetnacl.c", "tweetnacl/randombytes.c"
    )
    exec(
        "env", "ZIG_FLAGS=-target " .. zig_target,
        "cmake", "-GNinja",
        "-DCMAKE_SYSTEM_NAME=" .. o.cmake,
        "-DCMAKE_SYSTEM_PROCESSOR=" .. a.zig,
        "-DCMAKE_C_COMPILER=" .. zigcc,
        "-DCMAKE_AR=" .. zigar,
        "-DCMAKE_RANLIB=" .. zigranlib,
        "-DCMAKE_C_COMPILER_ABI=" .. o.abi,
        "-DLIBRESSL_SKIP_INSTALL=ON",
        "-DLIBRESSL_APPS=OFF",
        "-DLIBRESSL_TESTS=OFF",
        "-DBUILD_SHARED_LIBS=OFF",
        "-DCMAKE_BUILD_TYPE=MinSizeRel",
        "libressl", "-B", "lib/libressl-" .. zig_target
    )
    exec(
        "env", "ZIG_FLAGS=-target " .. zig_target,
        "DESTDIR=.", "ninja", "-C", "lib/libressl-" .. zig_target, "install"
    )
    exec(
        "env", "ZIG_FLAGS=-target " .. zig_target, "CC=" .. zigcc, "CFLAGS=" .. (o.curlcflags or ""),
        "cmake", "-GNinja",
        "-DCMAKE_CROSSCOMPILING=ON",
        "-DCMAKE_SYSTEM_NAME=" .. o.cmake,
        "-DCMAKE_SYSTEM_PROCESSOR=" .. a.zig,
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
        "-DZLIB_LIBRARY=" .. pwd .. "/lib/libz-" .. zig_target .. ".a",
        "-DZLIB_INCLUDE_DIR=" .. pwd .. "/zlib",
        "-DCMAKE_BUILD_TYPE=MinSizeRel",
        "curl", "-B", "lib/curl-" .. zig_target
    )
    exec(
        "env", "ZIG_FLAGS=-target " .. zig_target,
        "ninja", "-C", "lib/curl-" .. zig_target, "libcurl.a"
    )
    exec(
        "zig", "build-exe",
        "-target", zig_target,
        "-OReleaseSmall",
        "-fstrip",
        "-femit-bin=bin/yas-" .. o.zig .. "-" .. a.zig .. (o.bs or ""),
        "-Ilua/src",
        "-Icurl/include",
        "-Ilmdb",
        "-Itweetnacl",
        "-lc",
        "-Llib",
        "-Llib/curl-" .. zig_target .. "/lib",
        "-Llib/libressl-" .. zig_target .. "/crypto",
        "-Llib/libressl-" .. zig_target .. "/ssl",
        "-llua-" .. zig_target,
        "-llmdb-" .. zig_target,
        "-lzstd-" .. zig_target,
        "-ltnacl-" .. zig_target,
        "-lz-" .. zig_target,
        "-lcurl",
        "-lcrypto",
        "-lssl",
        "yas.zig"
    )
end

local keys = function(t)
    local r = {}
    for k, _ in pairs(t) do table.insert(r, k) end
    return r
end

if #arg > 0 then
    for _, os_arch in ipairs(arg) do
        local os, arch = os_arch:match("([^-]+)-([^-]+)")
        if (not os) or (not arch) then
            error("Invalid argument: expected $os-$arch instead of " .. os_arch)
        end
        local fos, farch = operating_systems[os], architectures[arch]
        if not fos then error("Invalid OS " .. os .. ": pass one of " .. table.concat(keys(operating_systems), ", ")) end
        if not farch then error("Invalid architecture " .. arch .. ": pass one of " .. table.concat(keys(architectures), ", ")) end
        build(fos, farch)
    end
else
    for _, os in pairs(operating_systems) do
        for _, arch in pairs(architectures) do
            build(os, arch)
        end
    end
end
