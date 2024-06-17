#include <lua.h>
#include <lualib.h>
#include <lauxlib.h>

lua_State* buildLua() {
    lua_State* L = luaL_newstate();
    luaL_openlibs(L);
    return L;
}
