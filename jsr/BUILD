load("@io_bazel_rules_go//go:def.bzl", "go_library")

cc_library(
    name = "quickjs",
    srcs = [
        "quickjs/cutils.c",
        "quickjs/libbf.c",
        "quickjs/libregexp.c",
        "quickjs/libunicode.c",
        "quickjs/quickjs.c",
        "quickjs/quickjs-libc.c",
    ],
    hdrs = [
        "quickjs/cutils.h",
        "quickjs/libbf.h",
        "quickjs/libregexp.h",
        "quickjs/libregexp-opcode.h",
        "quickjs/libunicode.h",
        "quickjs/libunicode-table.h",
        "quickjs/list.h",
        "quickjs/quickjs.h",
        "quickjs/quickjs-atom.h",
        "quickjs/quickjs-libc.h",
        "quickjs/quickjs-opcode.h",
    ],
    defines = [
        "CONFIG_VERSION='\"yas\"'",
    ],
    includes = ["quickjs"],
)

go_library(
    name = "jsr",
    srcs = ["jsr.go", "jsr.h"],
    cdeps = [":quickjs"],
    cgo = True,
    importpath = "yas.tools/jsr",
    visibility = ["//visibility:public"],
)
