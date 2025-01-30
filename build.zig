const std = @import("std");

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});

    const exe = b.addExecutable(.{
        .name = "yas",
        .root_source_file = b.path("main.zig"),
        .target = target,
        .optimize = optimize,
    });

    const win32 = b.addModule("win32", .{
        .root_source_file = b.path("libs/zigwin32/win32.zig"),
        .target = target,
        .optimize = optimize,
    });
    win32.addCMacro("WIDL_EXPLICIT_AGGREGATE_RETURNS", "1");

    exe.root_module.addImport("win32", win32);

    exe.linkSystemLibrary("d2d1");
    exe.linkSystemLibrary("d2d1");
    exe.linkSystemLibrary("dwrite");
    exe.linkSystemLibrary("user32");
    exe.linkSystemLibrary("gdi32");

    exe.subsystem = .Windows;
    b.installArtifact(exe);

    const run_cmd = b.addRunArtifact(exe);
    run_cmd.step.dependOn(b.getInstallStep());

    const run_step = b.step("run", "Run the app");
    run_step.dependOn(&run_cmd.step);
}
