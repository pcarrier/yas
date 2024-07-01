const lua = @cImport({
    @cInclude("lua.h");
    @cInclude("lualib.h");
    @cInclude("lauxlib.h");
});

const Package = struct {
    name: []const u8,
    load: lua.lua_CFunction,
    };

pub const packages = &[_]Package{
    Package{ .name = "coroutine", .load = lua.luaopen_coroutine },
    Package{ .name = "table", .load = lua.luaopen_table },
    Package{ .name = "io", .load = lua.luaopen_io },
    Package{ .name = "os", .load = lua.luaopen_os },
    Package{ .name = "string", .load = lua.luaopen_string },
    Package{ .name = "math", .load = lua.luaopen_math },
    Package{ .name = "utf8", .load = lua.luaopen_utf8 },
    Package{ .name = "debug", .load = lua.luaopen_debug },
};
