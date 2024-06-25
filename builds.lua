local utils = require("utils")
local exec, map = utils.exec, utils.map

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

return function(o, a)
    local pwd = os.getenv("PWD")
    local zigcc, zigar, zigranlib = pwd .. "/bin/zigcc", pwd .. "/bin/zigar", pwd .. "/bin/zigranlib"
    local zig_target = a .. "-" .. o.zig
    local exe = "out/yas-" .. o.short .. "-" .. a .. (o.bs or "")
    exec(
        "zig", "build-lib", "-femit-bin=lib/liblua-" .. zig_target .. ".a",
        "-OReleaseFast", "-fstrip", "-fsingle-threaded", "-lc", "-target", zig_target,
        "-Ilua/src",
        table.unpack(map(lua_files, function(f) return "deps/lua/src/" .. f .. ".c" end))
    )
    exec(
        "zig", "build-lib", "-femit-bin=lib/libz-" .. zig_target .. ".a",
        "-OReleaseFast", "-fstrip", "-fsingle-threaded", "-lc", "-target", zig_target,
        "-Ideps/zlib", "-DHAVE_SYS_TYPES_H", "-DHAVE_STDINT_H", "-DHAVE_STDDEF_H", "-DOFF64_T", "-D_LARGEFILE64_SOURCE=1",
        "-DZ_HAVE_UNISTD_H",
        table.unpack(map(zlib_files, function(f) return "deps/zlib/" .. f .. ".c" end))
    )
    exec(
        "zig", "build-lib", "-femit-bin=lib/libzstd-" .. zig_target .. ".a",
        "-OReleaseFast", "-fstrip", "-fsingle-threaded", "-lc", "-target", zig_target,
        table.unpack(map(zstd_files, function(f) return "deps/zstd/lib/" .. f .. ".c" end))
    )
    exec(
        "zig", "build-lib", "-femit-bin=lib/liblmdb-" .. zig_target .. ".a",
        "-OReleaseFast", "-fstrip", "-fsingle-threaded", "-lc", "-target", zig_target,
        "-I", "lmdb", "deps/lmdb/mdb.c", "deps/lmdb/midl.c"
    )
    exec(
        "zig", "build-lib", "-femit-bin=lib/libtnacl-" .. zig_target .. ".a",
        "-OReleaseFast", "-fstrip", "-fsingle-threaded", "-lc", "-target", zig_target,
        "deps/tweetnacl/tweetnacl.c", "deps/tweetnacl/randombytes.c"
    )
    exec(
        "env", "ZIG_FLAGS=-target " .. zig_target,
        "cmake", "-GNinja",
        "-DCMAKE_SYSTEM_NAME=" .. o.cmake,
        "-DCMAKE_SYSTEM_PROCESSOR=" .. a,
        "-DCMAKE_C_COMPILER=" .. zigcc,
        "-DCMAKE_AR=" .. zigar,
        "-DCMAKE_RANLIB=" .. zigranlib,
        "-DCMAKE_C_COMPILER_ABI=" .. o.abi,
        "-DLIBRESSL_SKIP_INSTALL=ON",
        "-DLIBRESSL_APPS=OFF",
        "-DLIBRESSL_TESTS=OFF",
        "-DBUILD_SHARED_LIBS=OFF",
        "-DCMAKE_BUILD_TYPE=MinSizeRel",
        "deps/libressl", "-B", "lib/libressl-" .. zig_target
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
        "-DCMAKE_SYSTEM_PROCESSOR=" .. a,
        "-DCMAKE_C_COMPILER=" .. zigcc,
        "-DCMAKE_AR=" .. zigar,
        "-DCMAKE_RANLIB=" .. zigranlib,
        "-DBUILD_SHARED_LIBS=OFF",
        "-DCURL_USE_OPENSSL=ON",
        "-DOPENSSL_USE_STATIC_LIBS=ON",
        "-DOPENSSL_ROOT_DIR=" .. pwd .. "/lib/libressl-" .. zig_target .. "/usr/local/lib",
        "-DOPENSSL_INCLUDE_DIR=" .. pwd .. "/deps/libressl/include",
        "-DCURL_ZSTD=ON",
        "-DZstd_LIBRARY=" .. pwd .. "/lib/libzstd-" .. zig_target .. ".a",
        "-DZstd_INCLUDE_DIR=" .. pwd .. "/deps/zstd/lib",
        "-DZLIB_LIBRARY=" .. pwd .. "/lib/libz-" .. zig_target .. ".a",
        "-DZLIB_INCLUDE_DIR=" .. pwd .. "/deps/zlib",
        "-DCMAKE_BUILD_TYPE=MinSizeRel",
        "deps/curl", "-B", "lib/curl-" .. zig_target
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
        "-femit-bin=" .. exe,
        "-Ideps/lua/src",
        "-Ideps/curl/include",
        "-Ideps/lmdb",
        "-Ideps/tweetnacl",
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
    return exe
end
