const builtin = @import("builtin");
const std = @import("std");
const c = @import("c.zig");
const license = @embedFile("LICENSE");
const osTag = builtin.os.tag;
const dirName = "yas.tools";
const defaultBaseURL = "https://oh.yas.tools";

fn dataDir(alloc: std.mem.Allocator) ![]u8 {
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

pub fn help(w: anytype, name: []u8) !void {
    try w.print("Usage: {s} [--help|--license|URL [args...]]", .{name});
}

pub fn main() !void {
    const alloc = std.heap.c_allocator;
    const stderr = std.io.getStdErr();
    const w = stderr.writer();
    const args = try std.process.argsAlloc(alloc);
    defer alloc.free(args);
    if (args.len == 1) {
        try help(w, args[0]);
        std.process.exit(1);
    }
    const cmd = args[1];
    if (std.mem.eql(u8, cmd, "--help")) {
        try help(w, args[0]);
        std.process.exit(0);
    } else if (std.mem.eql(u8, cmd, "--license")) {
        _ = try w.write(license);
        std.process.exit(0);
    }
    const data = try dataDir(alloc);
    defer alloc.free(data);
    try std.fs.cwd().makePath(data);
    const baseURL = std.process.getEnvVarOwned(alloc, "YAS_BASE") catch defaultBaseURL;
    try w.print("baseURL: {s}\n", .{baseURL});
    const lua = try c.Lua.init(&alloc);
    defer lua.deinit();
}
