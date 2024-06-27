local getenv = require("os").getenv
local utils = require("utils")
local table = require("table")
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

return function(o, a)
    local pwd = getenv("PWD")
    local zigcc, zigcpp, zigar, zigranlib =
        pwd .. "/bin/zigcc", pwd .. "/bin/zigcpp", pwd .. "/bin/zigar", pwd .. "/bin/zigranlib"
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
        "-DCMAKE_CXX_COMPILER=" .. zigcpp,
        "-DCMAKE_AR=" .. zigar,
        "-DCMAKE_RANLIB=" .. zigranlib,
        "-DCMAKE_C_COMPILER_ABI=" .. o.abi,
        "-DZSTD_BUILD_STATIC=ON",
        "-DZSTD_BUILD_SHARED=OFF",
        "-DZSTD_BUILD_PROGRAMS=OFF",
        "-DCMAKE_BUILD_TYPE=MinSizeRel",
        "deps/zstd/build/cmake", "-B", "lib/zstd-" .. zig_target
    )
    exec(
        "env", "ZIG_FLAGS=-target " .. zig_target,
        "DESTDIR=.", "ninja", "-C", "lib/zstd-" .. zig_target, "install"
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
        "-DBUILD_SHARED_LIBS=OFF",
        "-DCMAKE_BUILD_TYPE=MinSizeRel",
        "deps/brotli", "-B", "lib/brotli-" .. zig_target
    )
    exec(
        "env", "ZIG_FLAGS=-target " .. zig_target,
        "DESTDIR=.", "ninja", "-C", "lib/brotli-" .. zig_target, "install"
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
        "env", "ZIG_FLAGS=-target " .. zig_target,
        "cmake", "-GNinja",
        "-DCMAKE_SYSTEM_NAME=" .. o.cmake,
        "-DCMAKE_SYSTEM_PROCESSOR=" .. a,
        "-DCMAKE_C_COMPILER=" .. zigcc,
        "-DCMAKE_CXX_COMPILER=" .. zigcpp,
        "-DCMAKE_AR=" .. zigar,
        "-DCMAKE_RANLIB=" .. zigranlib,
        "-DCMAKE_C_COMPILER_ABI=" .. o.abi,
        "-DBUILD_SHARED_LIBS=OFF",
        "-DBUILD_STATIC_LIBS=ON",
        "-DOPENSSL_USE_STATIC_LIBS=ON",
        "-DOPENSSL_ROOT_DIR=" .. pwd .. "/lib/libressl-" .. zig_target .. "/usr/local/lib",
        "-DOPENSSL_INCLUDE_DIR=" .. pwd .. "/deps/libressl/include",
        "-DCMAKE_BUILD_TYPE=MinSizeRel",
        "deps/nghttp2", "-B", "lib/nghttp2-" .. zig_target
    )
    exec(
        "env", "ZIG_FLAGS=-target " .. zig_target,
        "DESTDIR=.", "ninja", "-C", "lib/nghttp2-" .. zig_target, "install"
    )
    exec(
        "env", "ZIG_FLAGS=-target " .. zig_target,
        "cmake", "-GNinja",
        "-DCMAKE_SYSTEM_NAME=" .. o.cmake,
        "-DCMAKE_SYSTEM_PROCESSOR=" .. a,
        "-DCMAKE_C_COMPILER=" .. zigcc,
        "-DCMAKE_CXX_COMPILER=" .. zigcpp,
        "-DCMAKE_AR=" .. zigar,
        "-DCMAKE_RANLIB=" .. zigranlib,
        "-DCMAKE_C_COMPILER_ABI=" .. o.abi,
        "-DCMAKE_C_BYTE_ORDER=stupid",
        "-DENABLE_SHARED_LIB=OFF",
        "-DENABLE_STATIC_LIB=ON",
        "-DCMAKE_BUILD_TYPE=MinSizeRel",
        "deps/nghttp3", "-B", "lib/nghttp3-" .. zig_target
    )
    exec(
        "env", "ZIG_FLAGS=-target " .. zig_target,
        "DESTDIR=.", "ninja", "-C", "lib/nghttp3-" .. zig_target, "install"
    )
    exec(
        "env", "ZIG_FLAGS=-target " .. zig_target,
        "cmake", "-GNinja",
        "-DCMAKE_SYSTEM_NAME=" .. o.cmake,
        "-DCMAKE_SYSTEM_PROCESSOR=" .. a,
        "-DCMAKE_C_COMPILER=" .. zigcc,
        "-DCMAKE_CXX_COMPILER=" .. zigcpp,
        "-DCMAKE_AR=" .. zigar,
        "-DCMAKE_RANLIB=" .. zigranlib,
        "-DCMAKE_C_COMPILER_ABI=" .. o.abi,
        "-DCMAKE_C_BYTE_ORDER=stupid",
        "-DENABLE_SHARED_LIB=OFF",
        "-DENABLE_STATIC_LIB=ON",
        "-DOPENSSL_USE_STATIC_LIBS=ON",
        "-DOPENSSL_ROOT_DIR=" .. pwd .. "/lib/libressl-" .. zig_target .. "/usr/local/lib",
        "-DOPENSSL_INCLUDE_DIR=" .. pwd .. "/deps/libressl/include",
        "-DLIBBROTLIENC_LIBRARY=" .. pwd .. "/lib/brotli-" .. zig_target .. "/usr/local/lib/libbrotlienc.a",
        "-DLIBBROTLIDEC_LIBRARY=" .. pwd .. "/lib/brotli-" .. zig_target .. "/usr/local/lib/libbrotlidec.a",
        "-DLIBBROTLIENC_INCLUDE_DIR=" .. pwd .. "/lib/brotli-" .. zig_target .. "/usr/local/include",
        "-DLIBBROTLIDEC_INCLUDE_DIR=" .. pwd .. "/lib/brotli-" .. zig_target .. "/usr/local/include",
        "-DCMAKE_BUILD_TYPE=MinSizeRel",
        "deps/ngtcp2", "-B", "lib/ngtcp2-" .. zig_target
    )
    exec(
        "env", "ZIG_FLAGS=-target " .. zig_target,
        "DESTDIR=.", "ninja", "-C", "lib/ngtcp2-" .. zig_target, "install"
    )
    exec(
        "env", "ZIG_FLAGS=-target " .. zig_target, "CC=" .. zigcc, "CFLAGS=" .. (o.cflags or ""),
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
        "-DCURL_BROTLI=ON",
        "-DBROTLICOMMON_LIBRARY=" .. pwd .. "/lib/brotli-" .. zig_target .. "/usr/local/lib/libbrotlicommon.a",
        "-DBROTLIDEC_LIBRARY=" .. pwd .. "/lib/brotli-" .. zig_target .. "/usr/local/lib/libbrotlidec.a",
        "-DBROTLI_INCLUDE_DIR=" .. pwd .. "/lib/brotli-" .. zig_target .. "/usr/local/include",
        "-DCURL_ZSTD=ON",
        "-DZstd_LIBRARY=" .. pwd .. "/lib/zstd-" .. zig_target .. "/usr/local/lib/libzstd.a",
        "-DZstd_INCLUDE_DIR=" .. pwd .. "/lib/zstd-" .. zig_target .. "/usr/local/include",
        "-DZLIB_LIBRARY=" .. pwd .. "/lib/libz-" .. zig_target .. ".a",
        "-DZLIB_INCLUDE_DIR=" .. pwd .. "/deps/zlib",
        "-DUSE_NGHTTP2=ON",
        "-DNGHTTP2_LIBRARY=" .. pwd .. "/lib/nghttp2-" .. zig_target .. "/usr/local/lib/libnghttp2.a",
        "-DNGHTTP2_INCLUDE_DIR=" .. pwd .. "/lib/nghttp2-" .. zig_target .. "/usr/local/include",
        "-DUSE_NGTCP2=ON",
        "-DNGHTTP3_LIBRARY=" .. pwd .. "/lib/nghttp3-" .. zig_target .. "/usr/local/lib/libnghttp3.a",
        "-DNGHTTP3_INCLUDE_DIR=" .. pwd .. "/lib/nghttp3-" .. zig_target .. "/usr/local/include",
        "-DNGTCP2_LIBRARY=" .. pwd .. "/lib/ngtcp2-" .. zig_target .. "/usr/local/lib/libngtcp2.a",
        "-Dngtcp2_crypto_quictls_LIBRARY=" ..
        pwd .. "/lib/ngtcp2-" .. zig_target .. "/usr/local/lib/libngtcp2_crypto_quictls.a",
        "-DNGTCP2_INCLUDE_DIR=" .. pwd .. "/lib/ngtcp2-" .. zig_target .. "/usr/local/include",
        "-DCMAKE_BUILD_TYPE=MinSizeRel",
        "deps/curl", "-B", "lib/curl-" .. zig_target
    )
    exec(
        "env", "ZIG_FLAGS=-target " .. zig_target,
        "DESTDIR=.", "ninja", "-C", "lib/curl-" .. zig_target, "install"
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
        "-Llib/zstd-" .. zig_target .. "/usr/local/lib",
        "-llua-" .. zig_target,
        "-llmdb-" .. zig_target,
        "-ltnacl-" .. zig_target,
        "-lz-" .. zig_target,
        "-lzstd",
        "-lcurl",
        "-lcrypto",
        "-lssl",
        "yas.zig"
    )
    return exe
end
