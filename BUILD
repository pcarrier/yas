load("@bazel_gazelle//:def.bzl", "gazelle")
load("@io_bazel_rules_go//go:def.bzl", "go_binary", "go_library")

gazelle(name = "gazelle")

go_binary(
    name = "yas",
    embed = [":yas.tools_lib"],
    importpath = "yas.tools",
    visibility = ["//visibility:public"],
)

go_library(
    name = "yas.tools_lib",
    srcs = ["main.go"],
    importpath = "yas.tools",
    visibility = ["//visibility:private"],
    deps = ["//cli"],
)
