const builtin = @import("builtin");
const c = @cImport({
    @cInclude("lua.h");
});

fn noop(_: ?*c.lua_State) callconv(.C) i32 {
    return 0;
}

export fn luaopen_buildalib(state: *c.lua_State) i32 {
    c.lua_createtable(state, 0, 3);
    const os = @tagName(builtin.os.tag);
    _ = c.lua_pushliteral(state, os);
    c.lua_setfield(state, -2, "os");
    const arch = @tagName(builtin.target.cpu.arch);
    _ = c.lua_pushliteral(state, arch);
    c.lua_setfield(state, -2, "arch");
    c.lua_pushcfunction(state, noop);
    c.lua_setfield(state, -2, "noop");
    return 1;
}
