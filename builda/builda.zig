const lua = @cImport({
    @cInclude("lua.h");
    @cInclude("lauxlib.h");
    @cInclude("lualib.h");
});
const builtin = @import("builtin");
const std = @import("std");
const StringBuilder = std.ArrayList(u8);
const bootstrap = @embedFile("builda.lua");

fn noop(_: ?*lua.lua_State) callconv(.C) i32 {
    return 0;
}

fn dump_writer(_: ?*lua.lua_State, p: ?*const anyopaque, sz: usize, ud: ?*anyopaque) callconv(.C) i32 {
    const s: [*]const u8 = @ptrCast(p);
    const str = s[0..sz];
    var sb: *StringBuilder = @ptrCast(@alignCast(ud));
    sb.appendSlice(str) catch {
        return 1;
    };
    return 0;
}

fn dump(state: ?*lua.lua_State) callconv(.C) i32 {
    const top = lua.lua_gettop(state);
    if (top < 1) {
        _ = lua.lua_pushliteral(state, "no function to dump");
        return lua.lua_error(state);
    }
    var strip = true;
    if (top > 1) {
        strip = lua.lua_toboolean(state, 2) != 0;
    }
    if (!lua.lua_isfunction(state, 1)) {
        _ = lua.lua_pushliteral(state, "not a function");
        return lua.lua_error(state);
    }
    lua.lua_settop(state, 1);
    var arena = std.heap.ArenaAllocator.init(std.heap.page_allocator);
    defer arena.deinit();
    var sb = StringBuilder.init(arena.allocator());
    defer sb.deinit();
    if (lua.lua_dump(state, dump_writer, @ptrCast(&sb), @intFromBool(strip)) != 0) {
        _ = lua.lua_pushliteral(state, "failed to dump function");
        return lua.lua_error(state);
    }
    const res = sb.toOwnedSlice() catch {
        _ = lua.lua_pushliteral(state, "failed to allocate memory");
        return lua.lua_error(state);
    };
    _ = lua.lua_pushlstring(state, res.ptr, res.len);
    return 1;
}

fn luaopen_buildalib(state: ?*lua.lua_State) callconv(.C) c_int {
    lua.lua_createtable(state, 0, 3);
    const os = @tagName(builtin.os.tag);
    _ = lua.lua_pushliteral(state, os);
    lua.lua_setfield(state, -2, "os");
    const arch = @tagName(builtin.target.cpu.arch);
    _ = lua.lua_pushliteral(state, arch);
    lua.lua_setfield(state, -2, "arch");
    lua.lua_pushcfunction(state, noop);
    lua.lua_setfield(state, -2, "noop");
    lua.lua_pushcfunction(state, dump);
    lua.lua_setfield(state, -2, "dump");
    return 1;
}

pub fn main() !void {
    const alloc = std.heap.c_allocator;
    const stderr = std.io.getStdErr();
    const w = stderr.writer();
    const args = try std.process.argsAlloc(alloc);

    const state = lua.luaL_newstate();
    if (state == null) {
        return error.Memory;
    }
    _ = lua.luaopen_base(state);
    _ = lua.lua_pop(state, 1);
    _ = lua.luaopen_package(state);
    _ = lua.lua_getfield(state, -1, "preload");
    const Package = struct {
        name: []const u8,
        load: lua.lua_CFunction,
    };
    const packages = &[_]Package{
        Package{ .name = "coroutine", .load = lua.luaopen_coroutine },
        Package{ .name = "table", .load = lua.luaopen_table },
        Package{ .name = "io", .load = lua.luaopen_io },
        Package{ .name = "os", .load = lua.luaopen_os },
        Package{ .name = "string", .load = lua.luaopen_string },
        Package{ .name = "math", .load = lua.luaopen_math },
        Package{ .name = "utf8", .load = lua.luaopen_utf8 },
        Package{ .name = "debug", .load = lua.luaopen_debug },
        Package{ .name = "buildalib", .load = luaopen_buildalib },
    };

    for (packages) |pkg| {
        lua.lua_pushcfunction(state, pkg.load);
        lua.lua_setfield(state, -2, pkg.name.ptr);
    }

    _ = lua.lua_pop(state, 3);

    lua.lua_newtable(state);
    var i: c_int = 0;
    for (args) |arg| {
        _ = lua.lua_pushlstring(state, arg.ptr, arg.len);
        _ = lua.lua_rawseti(state, -2, i);
        i += 1;
    }
    _ = lua.lua_setglobal(state, "args");

    if (lua.luaL_loadbufferx(state, bootstrap, bootstrap.len, "bootstrap", null) != lua.LUA_OK) {
        const err = lua.lua_tolstring(state, -1, null);
        try w.print("Error loading: {s}\n", .{err});
        std.process.exit(1);
    }
    if (lua.lua_pcallk(state, 0, 0, 0, 0, null) != lua.LUA_OK) {
        // Extract error and print it
        const err = lua.lua_tolstring(state, -1, null);
        try w.print("Error running: {s}\n", .{err});
        std.process.exit(1);
    }
}
