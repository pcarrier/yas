load("@io_bazel_rules_go//go:def.bzl", "go_library", "go_test")

go_library(
    name = "cli",
    srcs = [
        "run.go",
        "toolpath.go",
    ],
    importpath = "yas.tools/cli",
    visibility = ["//visibility:public"],
    deps = ["//jsr"],
)

go_test(
    name = "cli_test",
    srcs = ["toolpath_test.go"],
    embed = [":cli"],
)
