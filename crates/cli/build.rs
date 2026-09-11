fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    // Clap's generated command builders exceed the default 1 MiB Windows main
    // stack in debug builds, before even --help can run. Match Linux's usual
    // 8 MiB reserve; pages are committed on demand. Scope this to the CLI binary.
    let stack = if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        "/STACK:8388608"
    } else {
        "-Wl,--stack,8388608"
    };
    println!("cargo::rustc-link-arg-bin=yas={stack}");
}
