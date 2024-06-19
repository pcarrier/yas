import std/strformat

{.pragma: lua, importc: "lua_$1", header: "<lua.h>".}
{.pragma: laux, importc: "luaL_$1", header: "<lauxlib.h>".}

type 
  PState* = pointer
  Alloc* = proc (ud, theptr: pointer, osize, nsize: cint) {.cdecl.}

proc newstate*(): PState {.laux.}

let e = newstate()
echo &"Hello, {cast[uint](e)}!"
