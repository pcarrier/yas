const std = @import("std");
const Allocator = std.mem.Allocator;

const c = @cImport({
    @cInclude("lua.h");
    @cInclude("lauxlib.h");
    @cInclude("lualib.h");
    @cInclude("curl/curl.h");
    @cInclude("lmdb.h");
    @cInclude("tweetnacl.h");
});

pub const LuaState = *c.lua_State;

pub const Lua = opaque {
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

    pub fn init(allocator_ptr: *const Allocator) !*Lua {
        if (c.lua_newstate(alloc, @constCast(allocator_ptr))) |state| {
            return @ptrCast(state);
        } else return error.Memory;
    }

    pub fn deinit(lua: *Lua) void {
        c.lua_close(@ptrCast(lua));
    }
};
