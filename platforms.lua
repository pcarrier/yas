return {
  operating_systems = {
    macos = {
      short = "macos",
      cmake = "Darwin",
      zig = "macos",
      curlcflags = "-F/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/System/Library/Frameworks",
      abi = "",
    },
    linux = {
      short = "linux",
      cmake = "Linux",
      zig = "linux-musl",
      abi = "ELF",
    },
    windows = {
      short = "windows",
      cmake = "Windows",
      zig = "windows",
      bs = ".exe",
      abi = "",
  },
  },
  architectures = {
    'x86_64',
    'aarch64',
  },
}
