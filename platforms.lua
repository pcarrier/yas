return {
  operating_systems = {
    mac = {
      short = "mac",
      cmake = "Darwin",
      zig = "macos",
      curlcflags = "-F/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/System/Library/Frameworks",
      abi = "",
    },
    lin = {
      short = "linux",
      cmake = "Linux",
      zig = "linux-musl",
      abi = "ELF",
    },
    win = {
      short = "win",
      cmake = "Windows",
      zig = "windows",
      bs = ".exe",
      abi = "",
  },
  },
  architectures = {
    x64 = { zig = "x86_64", },
    a64 = { zig = "aarch64", },
  },
}
