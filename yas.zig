const lua = @cImport({
    @cInclude("lua.h");
    @cInclude("lualib.h");
    @cInclude("lauxlib.h");
});
const builtin = @import("builtin");
const std = @import("std");
const pkg = @import("./packages.zig");

const license = @embedFile("LICENSE");
const bootstrap = @embedFile("bootstrap.luac");

const Allocator = std.mem.Allocator;
const osTag = builtin.os.tag;
const dirName = "yas.tools";

const LuaErrorCode = enum(u3) {
    ok = lua.LUA_OK,
    run = lua.LUA_ERRRUN,
    mem = lua.LUA_ERRMEM,
    err = lua.LUA_ERRERR,
    syntax = lua.LUA_ERRSYNTAX,
    yield = lua.LUA_YIELD,
    file = lua.LUA_ERRFILE,
    };

const LuaError = error{ OK, RUN, MEM, ERR, SYNTAX, YIELD, FILE };

fn luaError(code: LuaErrorCode) LuaError {
    return switch (code) {
        .ok => LuaError.OK,
        .run => LuaError.RUN,
        .mem => LuaError.MEM,
        .err => LuaError.ERR,
        .syntax => LuaError.SYNTAX,
        .yield => LuaError.YIELD,
        .file => LuaError.FILE,
    };
}

pub const LuaState = *lua.lua_State;

pub const Runtime = opaque {
    const alignment = @alignOf(std.c.max_align_t);
    fn alloc(data: ?*anyopaque, ptr: ?*anyopaque, osize: usize, nsize: usize) callconv(.C) ?*align(alignment) anyopaque {
        const allocator_ptr: *Allocator = @ptrCast(@alignCast(data.?));
        if (@as(?[*]align(alignment) u8, @ptrCast(@alignCast(ptr)))) |prev_ptr| {
            const prev_slice = prev_ptr[0..osize];
            if (nsize == 0) {
                allocator_ptr.free(prev_slice);
                return null;
            }
            const new_ptr = allocator_ptr.realloc(prev_slice, nsize) catch return null;
            return new_ptr.ptr;
        } else if (nsize == 0) {
            return null;
        } else {
            const new_ptr = allocator_ptr.alignedAlloc(u8, alignment, nsize) catch return null;
            return new_ptr.ptr;
        }
    }

    pub fn init(allocator_ptr: *const Allocator) !*Runtime {
        if (lua.lua_newstate(alloc, @constCast(allocator_ptr))) |state| {
            return @ptrCast(state);
        } else return error.Memory;
    }

    pub fn deinit(rt: *Runtime) void {
        lua.lua_close(@ptrCast(rt));
    }

    pub fn run(rt: *Runtime, w: anytype, args: [][]u8, env: [][*:0]u8, data: []u8) !void {
        const state: LuaState = @ptrCast(rt);
        _ = lua.luaopen_base(state);
        _ = lua.luaopen_package(state);
        _ = lua.lua_pushvalue(state, -1);
        _ = lua.lua_setfield(state, 1, "package");
        _ = lua.lua_getfield(state, -1, "preload");
        for (pkg.packages) |p| {
            lua.lua_pushcfunction(state, p.load);
            lua.lua_setfield(state, -2, p.name.ptr);
        }
        _ = lua.lua_pop(state, 4);

        // Set _G["yas"] with env["VAR"] = val, arg[i], data
        lua.lua_newtable(state);
        lua.lua_newtable(state);

        var i: c_int = -1;
        for (args) |arg| {
            _ = lua.lua_pushstring(state, arg.ptr);
            _ = lua.lua_rawseti(state, -2, i);
            i += 1;
        }
        _ = lua.lua_setfield(state, -2, "args");
        lua.lua_newtable(state);
        for (env) |ev| {
            _ = lua.lua_pushstring(state, ev);
            _ = lua.lua_rawseti(state, -2, @intCast(lua.lua_rawlen(state, -2) + 1));
        }
        _ = lua.lua_setfield(state, -2, "env");
        _ = lua.lua_pushlstring(state, data.ptr, data.len);
        _ = lua.lua_setfield(state, -2, "data");
        _ = lua.lua_pushlstring(state, license.ptr, license.len);
        _ = lua.lua_setfield(state, -2, "license");
        lua.lua_setglobal(state, "yas");

        const loadCode: LuaErrorCode = @enumFromInt(lua.luaL_loadbufferx(state, bootstrap, bootstrap.len, "bootstrap", null));
        if (loadCode != .ok) return luaError(loadCode);
        const callCode: LuaErrorCode = @enumFromInt(lua.lua_pcallk(state, 0, 0, 0, 0, null));
        if (callCode != .ok) {
            const msg = lua.lua_tolstring(state, -1, null);
            try w.print("Error: {s}\n", .{msg});
        }
    }
    };

fn dataDir(alloc: Allocator) ![]u8 {
    if (osTag == .windows) {
        const appData = try std.process.getEnvVarOwned(alloc, "APPDATA");
        defer alloc.free(appData);
        return try std.fs.path.join(alloc, &[_][]const u8{ appData, dirName });
    } else {
        return std.process.getEnvVarOwned(alloc, "XDG_DATA_HOME") catch {
            const home = try std.process.getEnvVarOwned(alloc, "HOME");
            defer alloc.free(home);
            return try if (osTag == .macos)
                std.fs.path.join(alloc, &[_][]const u8{ home, "Library", "Application Support", dirName })
            else
                std.fs.path.join(alloc, &[_][]const u8{ home, ".local", "share", dirName });
        };
    }
}

pub fn main() !void {
    const alloc = std.heap.c_allocator;
    const stderr = std.io.getStdErr();
    const w = stderr.writer();
    const args = try std.process.argsAlloc(alloc);
    defer std.process.argsFree(alloc, args);
    const data = try dataDir(alloc);
    defer alloc.free(data);
    try std.fs.cwd().makePath(data);
    const rt = try Runtime.init(&alloc);
    defer rt.deinit();
    try rt.run(w, args, std.os.environ, data);
}
