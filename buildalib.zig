const builtin = @import("builtin");
const c = @cImport({
    @cInclude("lua.h");
    @cInclude("lauxlib.h");
    @cInclude("lualib.h");
});
const std = @import("std");
const StringBuilder = std.ArrayList(u8);

fn noop(_: ?*c.lua_State) callconv(.C) i32 {
    return 0;
}

fn dump_writer(_: ?*c.lua_State, p: ?*const anyopaque, sz: usize, ud: ?*anyopaque) callconv(.C) i32 {
    const s: [*]const u8 = @ptrCast(p);
    const str = s[0..sz];
    var sb: *StringBuilder = @ptrCast(@alignCast(ud));
    sb.appendSlice(str) catch {
        return 1;
    };
    return 0;
}

fn dump(state: ?*c.lua_State) callconv(.C) i32 {
    const top = c.lua_gettop(state);
    if (top < 1) {
        _ = c.lua_pushliteral(state, "no function to dump");
        return c.lua_error(state);
    }
    var strip = true;
    if (top > 1) {
        strip = c.lua_toboolean(state, 2) != 0;
    }
    if (!c.lua_isfunction(state, 1)) {
        _ = c.lua_pushliteral(state, "not a function");
        return c.lua_error(state);
    }
    c.lua_settop(state, 1);
    var arena = std.heap.ArenaAllocator.init(std.heap.page_allocator);
    defer arena.deinit();
    var sb = StringBuilder.init(arena.allocator());
    defer sb.deinit();
    if (c.lua_dump(state, dump_writer, @ptrCast(&sb), @intFromBool(strip)) != 0) {
        _ = c.lua_pushliteral(state, "failed to dump function");
        return c.lua_error(state);
    }
    const res = sb.toOwnedSlice() catch {
        _ = c.lua_pushliteral(state, "failed to allocate memory");
        return c.lua_error(state);
    };
    _ = c.lua_pushlstring(state, res.ptr, res.len);
    return 1;
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
    c.lua_pushcfunction(state, dump);
    c.lua_setfield(state, -2, "dump");
    return 1;
}
