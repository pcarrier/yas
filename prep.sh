#!/bin/sh
set -ex
mkdir -p lib out
export LUA_CPATH=./lib/?.so
zig build-lib -femit-bin=lib/buildalib.so -O ReleaseSmall -fstrip -dynamic -Ideps/lua/src -fallow-shlib-undefined buildalib.zig
zig build-exe -femit-bin=bin/builda -O ReleaseSmall -fstrip -lc -DLUA_USE_DLOPEN \
deps/lua/src/lua.c \
deps/lua/src/lapi.c \
deps/lua/src/lcode.c \
deps/lua/src/lctype.c \
deps/lua/src/ldebug.c \
deps/lua/src/ldo.c \
deps/lua/src/ldump.c \
deps/lua/src/lfunc.c \
deps/lua/src/lgc.c \
deps/lua/src/linit.c \
deps/lua/src/llex.c \
deps/lua/src/lmem.c \
deps/lua/src/lobject.c \
deps/lua/src/lopcodes.c \
deps/lua/src/lparser.c \
deps/lua/src/lstate.c \
deps/lua/src/lstring.c \
deps/lua/src/ltable.c \
deps/lua/src/ltm.c \
deps/lua/src/lundump.c \
deps/lua/src/lvm.c \
deps/lua/src/lzio.c \
deps/lua/src/lauxlib.c \
deps/lua/src/lbaselib.c \
deps/lua/src/lcorolib.c \
deps/lua/src/ldblib.c \
deps/lua/src/liolib.c \
deps/lua/src/lmathlib.c \
deps/lua/src/loadlib.c \
deps/lua/src/loslib.c \
deps/lua/src/lstrlib.c \
deps/lua/src/ltablib.c \
deps/lua/src/lutf8lib.c
