const builtin = @import("builtin");
const c = @cImport({
    @cInclude("lua.h");
});

export fn luaopen_local(state: *c.lua_State) i32 {
    c.lua_newtable(state);
    const os = @tagName(builtin.os.tag);
    _ = c.lua_pushliteral(state, os);
    c.lua_setfield(state, -2, "os");
    const arch = @tagName(builtin.target.cpu.arch);
    _ = c.lua_pushliteral(state, arch);
    c.lua_setfield(state, -2, "arch");
    return 1;
}
