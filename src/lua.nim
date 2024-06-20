type 
  PState* = pointer
  Alloc* = proc (ud, theptr: pointer, osize, nsize: cint) {.cdecl.}

proc newstate*(): PState {.importc: "luaL_$1", header: "<lauxlib.h>".}
proc openlibs*(l: PState): void {.importc: "luaL_$1", header: "<lualib.h>".}
