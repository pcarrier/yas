mod cli;
mod completion;
mod events_human;
mod forward;
mod generate;
mod grep;
mod interactive;
mod share;
mod socks;
mod terminal_args;
mod transport;
mod uplink;
mod yas_client;
mod yas_core;
mod yas_events;
mod yas_extension;
mod yas_fs;
mod yas_git;
mod yas_kv;
mod yas_lsp;
mod yas_media;
mod yas_native;
mod yas_net;
mod yas_process;
mod yas_remotes;
mod yas_selection;
mod yas_surface;
mod yas_terminal;

use clap::Parser;
use cli::{Cli, ClipboardCommand, Command, RemoteCommand, SurfaceCommand, TerminalCommand};

// glibc malloc retains freed memory in per-thread arenas (up to 8 per core);
// with one tokio worker per core this inflates RSS by hundreds of MB under
// streaming load. mimalloc returns memory far more aggressively.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// Bound arenas used by native libraries that bypass Rust's allocator.
///
/// NVENC/CUDA, notify, and other C dependencies still call glibc malloc.
/// Its default permits up to eight arenas per CPU, and a heavily threaded
/// server can leave hundreds of 64 MiB arena mappings resident after bursts.
/// Rust allocations use mimalloc, so four native arenas provide concurrency
/// without multiplying retained memory by the server's thread count. An
/// explicit `MALLOC_ARENA_MAX` remains authoritative.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn limit_native_malloc_arenas() {
    if std::env::var_os("MALLOC_ARENA_MAX").is_none() {
        // SAFETY: `mallopt` is process-global and this is the first action in
        // main, before yas creates any worker threads.
        unsafe {
            libc::mallopt(libc::M_ARENA_MAX, 4);
        }
    }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn limit_native_malloc_arenas() {}

fn main() {
    limit_native_malloc_arenas();

    // ProxyDaemon must run synchronously — yas_proxy::run() builds its own
    // tokio runtime, which panics if called from within an existing one.
    // Detect this subcommand before entering the async runtime. Account for
    // global option values, but stop at the actual subcommand: a verbatim
    // argument after `@name` must never launch the daemon.
    if proxy_daemon_requested(std::env::args().skip(1)) {
        yas_proxy::run(false);
        return;
    }

    // `--license` works like `--help`: a bare top-level flag, handled before
    // clap since every other invocation requires a subcommand.
    if std::env::args().nth(1).as_deref() == Some("--license") {
        print!("{}", cli::license_text());
        return;
    }

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime")
        .block_on(async_main());
}

fn proxy_daemon_requested<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut args = args.into_iter();
    while let Some(argument) = args.next() {
        let argument = argument.as_ref();
        if matches!(argument, "--on" | "--hub") {
            let _ = args.next();
            continue;
        }
        if argument.starts_with("--on=") || argument.starts_with("--hub=") {
            continue;
        }
        if argument == "proxy-daemon" {
            return true;
        }
        if !argument.starts_with('-') {
            return false;
        }
    }
    false
}

async fn async_main() {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    if completion::run_if_requested(std::env::args().skip(1)).await {
        return;
    }

    let cli = Cli::parse();

    if cli.advertised_command_json && !matches!(&cli.command, Command::External(_)) {
        eprintln!("yas: root --json is only valid before an extension command namespace (@name)");
        std::process::exit(2);
    }

    match cli.command {
        Command::Terminal { command } => {
            let cmd = command.unwrap_or(TerminalCommand::List);
            let conn = &cli.connect;
            let result = match cmd {
                TerminalCommand::List => {
                    yas_terminal::cmd_list(conn.on.as_deref(), &conn.hub).await
                }
                TerminalCommand::Start {
                    command,
                    shell,
                    cwd,
                    env,
                    tag,
                    rows,
                    cols,
                    wait,
                    timeout,
                    deadline,
                } => {
                    let request = terminal_args::StartRequest {
                        tag,
                        command,
                        shell,
                        cwd,
                        env,
                        rows,
                        cols,
                        deadline,
                    };
                    let start_result =
                        yas_terminal::cmd_start(conn.on.as_deref(), &conn.hub, request).await;
                    if wait {
                        let pty_id = match start_result {
                            Ok(id) => id,
                            Err(e) => {
                                eprintln!("yas: {e}");
                                std::process::exit(1);
                            }
                        };
                        match yas_terminal::cmd_wait(
                            conn.on.as_deref(),
                            &conn.hub,
                            pty_id,
                            timeout.unwrap(),
                            None,
                        )
                        .await
                        {
                            Ok(code) => std::process::exit(code),
                            Err(e) => {
                                eprintln!("yas: {e}");
                                std::process::exit(1);
                            }
                        }
                    }
                    start_result.map(|_| ())
                }
                TerminalCommand::Show {
                    id,
                    ansi,
                    rows,
                    cols,
                } => {
                    yas_terminal::cmd_show(conn.on.as_deref(), &conn.hub, id, ansi, rows, cols)
                        .await
                }
                TerminalCommand::Cwd { id } => {
                    yas_terminal::cmd_cwd(conn.on.as_deref(), &conn.hub, id).await
                }
                TerminalCommand::History {
                    id,
                    from_start,
                    from_end,
                    limit,
                    since,
                    max_bytes,
                    json,
                    ansi,
                    rows,
                    cols,
                } => {
                    yas_terminal::cmd_history(
                        conn.on.as_deref(),
                        &conn.hub,
                        id,
                        from_start,
                        from_end,
                        limit,
                        since,
                        max_bytes,
                        json,
                        ansi,
                        rows,
                        cols,
                    )
                    .await
                }
                TerminalCommand::Journal {
                    id,
                    from,
                    limit,
                    json,
                } => match yas_terminal::cmd_journal(
                    conn.on.as_deref(),
                    &conn.hub,
                    id,
                    from,
                    limit,
                    json,
                )
                .await
                {
                    Ok(code) => std::process::exit(code),
                    Err(e) => {
                        eprintln!("yas: {e}");
                        std::process::exit(1);
                    }
                },
                TerminalCommand::Output {
                    id,
                    index,
                    wait,
                    max_bytes,
                    json,
                } => match yas_terminal::cmd_output(
                    conn.on.as_deref(),
                    &conn.hub,
                    id,
                    index,
                    wait,
                    max_bytes,
                    json,
                )
                .await
                {
                    Ok(code) => std::process::exit(code),
                    Err(e) => {
                        eprintln!("yas: {e}");
                        std::process::exit(1);
                    }
                },
                TerminalCommand::Send { id, text } => {
                    let text = if text == "-" {
                        use std::io::Read;
                        let mut buf = String::new();
                        std::io::stdin().read_to_string(&mut buf).unwrap_or(0);
                        buf
                    } else {
                        text
                    };
                    yas_terminal::cmd_send(conn.on.as_deref(), &conn.hub, id, text).await
                }
                TerminalCommand::Mouse {
                    id,
                    event,
                    col,
                    row,
                    button,
                } => {
                    yas_terminal::cmd_mouse(
                        conn.on.as_deref(),
                        &conn.hub,
                        id,
                        &event,
                        col,
                        row,
                        &button,
                    )
                    .await
                }
                TerminalCommand::Click {
                    id,
                    col,
                    row,
                    button,
                } => {
                    yas_terminal::cmd_terminal_click(
                        conn.on.as_deref(),
                        &conn.hub,
                        id,
                        col,
                        row,
                        &button,
                    )
                    .await
                }
                TerminalCommand::Wait {
                    id,
                    timeout,
                    pattern,
                } => {
                    match yas_terminal::cmd_wait(
                        conn.on.as_deref(),
                        &conn.hub,
                        id,
                        timeout,
                        pattern,
                    )
                    .await
                    {
                        Ok(code) => std::process::exit(code),
                        Err(e) => {
                            eprintln!("yas: {e}");
                            std::process::exit(1);
                        }
                    }
                }
                TerminalCommand::Restart { id } => {
                    yas_terminal::cmd_restart(conn.on.as_deref(), &conn.hub, id).await
                }
                TerminalCommand::Deadline { id, seconds } => {
                    yas_terminal::cmd_deadline(conn.on.as_deref(), &conn.hub, id, seconds).await
                }
                TerminalCommand::Kill { id, signal } => {
                    yas_terminal::cmd_kill(conn.on.as_deref(), &conn.hub, id, &signal).await
                }
                TerminalCommand::Close { id } => {
                    yas_terminal::cmd_close(conn.on.as_deref(), &conn.hub, id).await
                }
                TerminalCommand::Attach { id } => {
                    match yas_terminal::cmd_attach(conn.on.as_deref(), &conn.hub, id).await {
                        Ok(code) => std::process::exit(code),
                        Err(e) => Err(e),
                    }
                }
                TerminalCommand::Resize { id, cols, rows } => {
                    yas_terminal::cmd_resize(conn.on.as_deref(), &conn.hub, id, cols, rows).await
                }
                TerminalCommand::Grep {
                    pattern,
                    ids,
                    regexps,
                    pattern_files,
                    fixed_strings,
                    word_regexp,
                    line_regexp,
                    ignore_case,
                    case_sensitive,
                    smart_case,
                    invert_match,
                    multiline,
                    multiline_dotall,
                    after_context,
                    before_context,
                    context,
                    context_separator,
                    no_context_separator,
                    line_number,
                    no_line_number,
                    with_filename,
                    no_filename,
                    heading,
                    no_heading,
                    column,
                    count,
                    count_matches,
                    files_with_matches,
                    files_without_match,
                    only_matching,
                    max_count,
                    passthru,
                    vimgrep,
                    json,
                    pretty,
                    null,
                    color,
                    field_context_separator,
                    field_match_separator,
                    quiet,
                    no_messages,
                    stats,
                    stop_on_nonmatch,
                    sort,
                    sortr,
                    tag,
                    title,
                    running,
                    exited,
                    all,
                } => {
                    let opts = match grep::Opts::from_cli(
                        pattern,
                        ids,
                        regexps,
                        pattern_files,
                        fixed_strings,
                        word_regexp,
                        line_regexp,
                        ignore_case,
                        case_sensitive,
                        smart_case,
                        multiline,
                        multiline_dotall,
                        after_context,
                        before_context,
                        context,
                        context_separator,
                        no_context_separator,
                        line_number,
                        no_line_number,
                        with_filename,
                        no_filename,
                        heading,
                        no_heading,
                        column,
                        count,
                        count_matches,
                        files_with_matches,
                        files_without_match,
                        only_matching,
                        max_count,
                        passthru,
                        vimgrep,
                        json,
                        pretty,
                        null,
                        color,
                        field_context_separator,
                        field_match_separator,
                        quiet,
                        no_messages,
                        stats,
                        stop_on_nonmatch,
                        invert_match,
                        sort,
                        sortr,
                        tag,
                        title,
                        running,
                        exited,
                        all,
                    ) {
                        Ok(o) => o,
                        Err(e) => {
                            eprintln!("yas: {e}");
                            std::process::exit(2);
                        }
                    };
                    match yas_terminal::cmd_grep(conn.on.as_deref(), &conn.hub, opts).await {
                        Ok(code) => std::process::exit(code),
                        Err(e) => {
                            eprintln!("yas: {e}");
                            std::process::exit(2);
                        }
                    }
                }
                TerminalCommand::Record {
                    id,
                    output,
                    frames,
                    duration,
                } => {
                    yas_terminal::cmd_record(
                        conn.on.as_deref(),
                        &conn.hub,
                        id,
                        output,
                        frames,
                        duration,
                    )
                    .await
                }
            };
            if let Err(e) = result {
                eprintln!("yas: {e}");
                std::process::exit(1);
            }
        }
        Command::Client { command } => {
            let conn = &cli.connect;
            if let Err(e) = yas_client::dispatch(conn.on.as_deref(), &conn.hub, command).await {
                eprintln!("yas: {e}");
                std::process::exit(1);
            }
        }
        Command::Events { command } => {
            let conn = &cli.connect;
            if let Err(error) = yas_events::dispatch(conn.on.as_deref(), &conn.hub, command).await {
                eprintln!("yas: {error}");
                std::process::exit(1);
            }
        }
        Command::Surface { command } => {
            let cmd = command.unwrap_or(SurfaceCommand::List);
            let conn = &cli.connect;
            let result = match cmd {
                SurfaceCommand::List => yas_surface::cmd_list(conn.on.as_deref(), &conn.hub).await,
                SurfaceCommand::Close { id } => {
                    yas_surface::cmd_close(conn.on.as_deref(), &conn.hub, id).await
                }
                SurfaceCommand::Capture {
                    id,
                    output,
                    format,
                    quality,
                    width,
                    height,
                    scale,
                } => {
                    yas_surface::cmd_capture(
                        conn.on.as_deref(),
                        &conn.hub,
                        id,
                        output,
                        format,
                        quality,
                        width,
                        height,
                        scale,
                    )
                    .await
                }
                SurfaceCommand::Click { id, x, y, button } => {
                    yas_surface::cmd_click(conn.on.as_deref(), &conn.hub, id, x, y, &button).await
                }
                SurfaceCommand::Key { id, key } => {
                    yas_surface::cmd_key(conn.on.as_deref(), &conn.hub, id, &key).await
                }
                SurfaceCommand::Scroll {
                    id,
                    amount,
                    horizontal,
                    smooth,
                } => {
                    yas_surface::cmd_scroll(
                        conn.on.as_deref(),
                        &conn.hub,
                        id,
                        amount,
                        horizontal,
                        smooth,
                    )
                    .await
                }
                SurfaceCommand::Focus { id } => {
                    yas_surface::cmd_focus(conn.on.as_deref(), &conn.hub, id).await
                }
                SurfaceCommand::Text { id, text } => {
                    yas_surface::cmd_text(conn.on.as_deref(), &conn.hub, id, &text).await
                }
                SurfaceCommand::Type { id, text } => {
                    yas_surface::cmd_type(conn.on.as_deref(), &conn.hub, id, &text).await
                }
                SurfaceCommand::Record {
                    id,
                    output,
                    frames,
                    duration,
                    codec,
                    size,
                    encode_size,
                    fps,
                    timing,
                } => {
                    yas_surface::cmd_record(
                        conn.on.as_deref(),
                        &conn.hub,
                        id,
                        output,
                        frames,
                        duration,
                        codec,
                        size,
                        encode_size,
                        fps,
                        timing,
                    )
                    .await
                }
            };
            if let Err(e) = result {
                eprintln!("yas: {e}");
                std::process::exit(1);
            }
        }
        Command::Media { command } => {
            let conn = &cli.connect;
            if let Err(error) = yas_media::dispatch(conn.on.as_deref(), &conn.hub, command).await {
                eprintln!("yas: {error}");
                std::process::exit(1);
            }
        }
        Command::Clipboard { command } => {
            let cmd = command.unwrap_or(ClipboardCommand::List);
            let conn = &cli.connect;
            let result = match cmd {
                ClipboardCommand::List => {
                    yas_selection::cmd_list(conn.on.as_deref(), &conn.hub).await
                }
                ClipboardCommand::Get { mime } => {
                    yas_selection::cmd_get(conn.on.as_deref(), &conn.hub, &mime).await
                }
                ClipboardCommand::Set {
                    mime,
                    primary,
                    text,
                } => {
                    yas_selection::cmd_set(conn.on.as_deref(), &conn.hub, &mime, primary, text)
                        .await
                }
            };
            if let Err(e) = result {
                eprintln!("yas: {e}");
                std::process::exit(1);
            }
        }
        Command::Fs { command } => {
            let conn = &cli.connect;
            match yas_fs::dispatch(conn.on.as_deref(), &conn.hub, command).await {
                Ok(code) => std::process::exit(code),
                Err(e) => {
                    eprintln!("yas: {e}");
                    std::process::exit(1);
                }
            }
        }
        Command::Git { command } => {
            let conn = &cli.connect;
            match yas_git::dispatch(conn.on.as_deref(), &conn.hub, command).await {
                Ok(code) => std::process::exit(code),
                Err(e) => {
                    eprintln!("yas: {e}");
                    std::process::exit(1);
                }
            }
        }
        Command::Kv { command } => {
            let conn = &cli.connect;
            match yas_kv::dispatch(conn.on.as_deref(), &conn.hub, command).await {
                Ok(code) => std::process::exit(code),
                Err(e) => {
                    eprintln!("yas: {e}");
                    std::process::exit(1);
                }
            }
        }
        Command::Lsp { command } => {
            let conn = &cli.connect;
            match yas_lsp::dispatch(conn.on.as_deref(), &conn.hub, command).await {
                Ok(code) => std::process::exit(code),
                Err(e) => {
                    eprintln!("yas: {e}");
                    std::process::exit(2);
                }
            }
        }
        Command::Extension { command } => {
            let conn = &cli.connect;
            let result = yas_extension::dispatch(conn.on.as_deref(), &conn.hub, command).await;
            match result {
                Ok(code) => std::process::exit(code),
                Err(e) => {
                    eprintln!("yas: {e}");
                    std::process::exit(1);
                }
            }
        }
        Command::Run(args) => {
            let conn = &cli.connect;
            match yas_process::run(conn.on.as_deref(), &conn.hub, args).await {
                Ok(code) => std::process::exit(code),
                Err(e) => {
                    eprintln!("yas: {e}");
                    std::process::exit(1);
                }
            }
        }
        Command::External(tokens) => {
            // Reject unknown commands and oversized argument vectors before a
            // bad invocation can cause a remote connection.
            let result = match yas_extension::parse_advertised_command(tokens) {
                Ok((name, args)) => {
                    let conn = &cli.connect;
                    yas_extension::dispatch_advertised_command(
                        conn.on.as_deref(),
                        &conn.hub,
                        name,
                        args,
                        cli.advertised_command_json,
                    )
                    .await
                }
                Err(e) => Err(e),
            };
            match result {
                Ok(code) => std::process::exit(code),
                Err(e) => {
                    eprintln!("yas: {e}");
                    std::process::exit(1);
                }
            }
        }
        Command::Remote { command } => {
            let cmd = command.unwrap_or(RemoteCommand::List { reveal: false });
            let conn = &cli.connect;
            if let Err(error) = cmd_remote(cmd, conn.on.as_deref(), &conn.hub).await {
                eprintln!("yas: {error}");
                std::process::exit(1);
            }
        }
        Command::Quit => {
            let conn = &cli.connect;
            if let Err(e) = yas_core::cmd_quit(conn.on.as_deref(), &conn.hub).await {
                eprintln!("yas: {e}");
                std::process::exit(1);
            }
        }
        Command::Server {
            name,
            socket,
            shell_flags,
            scrollback,
            #[cfg(unix)]
            fd_channel,
            export_sock,
            inject_path,
            max_ptys,
            surface_encoders,
            camera_codecs,
            microphone_codecs,
            allow_forward,
            allow_forward_insecure,
            no_persistent_extensions,
            edge,
            share,
            deployment,
            verbose,
            no_processes,
        } => {
            let deployment = match deployment.into_overrides() {
                Ok(deployment) => deployment,
                Err(error) => {
                    eprintln!("yas server: {error}");
                    std::process::exit(2);
                }
            };
            if let Err(error) = yas_server::configure_deployment(deployment) {
                eprintln!("yas server: {error}");
                std::process::exit(2);
            }
            // A typed list that does not parse is a mistake worth stopping
            // for; the environment fallbacks inside `defaults()` stay lenient,
            // since a stale export should not make the server unbootable.
            let fail = |error: String| -> ! {
                eprintln!("yas server: {error}");
                std::process::exit(2);
            };
            let surface_encoders = match surface_encoders {
                Some(list) => yas_server::SurfaceEncoderPreference::parse_list(&list)
                    .unwrap_or_else(|error| fail(error)),
                None => yas_server::SurfaceEncoderPreference::defaults(),
            };
            let media_codecs = {
                let defaults = yas_server::MediaCodecPolicy::defaults();
                yas_server::MediaCodecPolicy {
                    camera: match camera_codecs {
                        Some(list) => yas_server::MediaCodecPolicy::parse_camera(&list)
                            .unwrap_or_else(|error| fail(error)),
                        None => defaults.camera,
                    },
                    microphone: match microphone_codecs {
                        Some(list) => yas_server::MediaCodecPolicy::parse_microphone(&list)
                            .unwrap_or_else(|error| fail(error)),
                        None => defaults.microphone,
                    },
                }
            };
            let ipc_path_override = socket.or_else(|| std::env::var("YAS_SOCK").ok());
            let ipc_path_is_automatic = ipc_path_override.is_none();
            let automatic_ipc_template = yas_server::default_ipc_path_template();
            let ipc_path =
                ipc_path_override.unwrap_or_else(|| yas_server::default_ipc_path_for(&name));

            #[cfg(unix)]
            let shell_default = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
            #[cfg(windows)]
            let shell_default = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into());

            #[cfg(unix)]
            let flags_default = "li";
            #[cfg(windows)]
            let flags_default = "";

            let vaapi_device =
                std::env::var("YAS_VAAPI_DEVICE").unwrap_or_else(|_| "/dev/dri/renderD128".into());
            let compositor_device =
                yas_server::compositor_device_from_env(&surface_encoders, &vaapi_device);

            let config = yas_server::Config {
                name,
                shell: shell_default,
                shell_flags: shell_flags
                    .or_else(|| std::env::var("YAS_SHELL_FLAGS").ok())
                    .unwrap_or_else(|| flags_default.into()),
                scrollback: scrollback
                    .or_else(|| {
                        std::env::var("YAS_SCROLLBACK")
                            .ok()
                            .and_then(|s| s.parse().ok())
                    })
                    .unwrap_or(10_000),
                ipc_path,
                ipc_path_is_automatic,
                automatic_ipc_template,
                surface_encoders,
                surface_encoding: yas_server::SurfaceEncoding {
                    bandwidth: std::env::var("YAS_SURFACE_BANDWIDTH")
                        .ok()
                        .and_then(|v| yas_server::SurfaceBandwidth::parse(&v))
                        .unwrap_or_default(),
                    speed: std::env::var("YAS_SURFACE_SPEED")
                        .ok()
                        .and_then(|v| yas_server::SurfaceSpeed::parse(&v))
                        .unwrap_or_default(),
                },
                chroma: yas_server::ChromaSubsampling::from_env(),
                media_codecs,
                vaapi_device,
                compositor_device,
                #[cfg(unix)]
                fd_channel: fd_channel.or_else(|| {
                    std::env::var("YAS_FD_CHANNEL")
                        .ok()
                        .and_then(|s| s.parse().ok())
                }),
                verbose: verbose
                    || std::env::var("YAS_VERBOSE")
                        .ok()
                        .map(|v| v == "1")
                        .unwrap_or(false),
                processes: !no_processes
                    && !std::env::var("YAS_PROCESS").is_ok_and(|value| value == "0"),
                // Both default to 0 (unlimited), which is the right default:
                // a client that can open a PTY can already spend the machine's
                // resources from inside it, so these are an operator sanity
                // bound against runaway automation, not a security control.
                // They were hardcoded to 0 with no way to set them, which made
                // the enforcement in yas-server dead code.
                max_connections: env_usize("YAS_MAX_CONNECTIONS"),
                // The flag takes precedence over the env var; absent both, the
                // env default of 0 stands.
                max_ptys: max_ptys.unwrap_or_else(|| env_usize("YAS_MAX_PTYS")),
                ping_interval: std::time::Duration::from_secs(10),
                skip_compositor: std::env::var("YAS_SKIP_COMPOSITOR")
                    .ok()
                    .map(|v| v == "1")
                    .unwrap_or(false),
                export_sock: export_sock
                    || std::env::var("YAS_EXPORT_SOCK")
                        .ok()
                        .map(|v| v == "1")
                        .unwrap_or(false),
                inject_path: inject_path
                    || std::env::var("YAS_INJECT_PATH")
                        .ok()
                        .map(|v| v == "1")
                        .unwrap_or(false),
                allow_forward,
                allow_forward_insecure,
                allow_persistent_extensions: !no_persistent_extensions
                    && !std::env::var("YAS_ALLOW_EXT_PERSIST").is_ok_and(|value| value == "0"),
            };
            // The browser edge and the WebRTC share used to be separate
            // processes dialling this one's socket. Asked for here they are
            // started with a door straight into it, which is one unit to
            // manage, one passphrase to set, and no proxy daemon pooling
            // connections to a server that is already in the same address
            // space.
            let hub = cli.connect.hub.clone();
            let hosted: Option<yas_server::HostedServices> =
                (edge || share || env_flag("YAS_EDGE") || env_flag("YAS_SHARE")).then(|| {
                    let edge = edge || env_flag("YAS_EDGE");
                    let share = share || env_flag("YAS_SHARE");
                    Box::new(move |endpoint: yas_server::LocalEndpoint| {
                        if edge {
                            tokio::spawn(hosted_edge(endpoint.clone()));
                        }
                        if share {
                            tokio::spawn(share::run(share::Options {
                                hub,
                                // A hosted share's stdout is a service log, and
                                // the URL it would print carries the passphrase.
                                // Say nothing unless asked: whoever configured
                                // the passphrase can already derive the URL.
                                quiet: !std::env::var("YAS_SHARE_QUIET")
                                    .is_ok_and(|value| value == "0"),
                                verbose: env_flag("YAS_SHARE_VERBOSE"),
                                hosted: Some(hosted_connector(endpoint)),
                            }));
                        }
                    }) as yas_server::HostedServices
                });
            yas_server::run_hosted(config, hosted).await;
        }
        Command::Uplink { url } => {
            if let Err(e) = uplink::cmd_uplink(url).await {
                eprintln!("yas: {e}");
                std::process::exit(1);
            }
        }
        Command::Share { quiet, verbose } => {
            share::run(share::Options {
                hub: cli.connect.hub.clone(),
                quiet,
                verbose,
                hosted: None,
            })
            .await;
        }
        Command::Install { host } => match host {
            Some(host) => {
                if let Err(e) = cmd_install(&host).await {
                    eprintln!("yas: {e}");
                    std::process::exit(1);
                }
            }
            None => {
                println!("# Linux / macOS");
                println!("curl -sf https://yas.run | sh");
                println!();
                println!("# Windows (PowerShell)");
                println!("irm https://yas.run/install.ps1 | iex");
            }
        },
        Command::Upgrade => {
            if let Err(e) = cmd_upgrade().await {
                eprintln!("yas: {e}");
                std::process::exit(1);
            }
        }
        Command::HashPassphrase { value } => {
            if let Err(e) = cmd_hash_passphrase(value) {
                eprintln!("yas: {e}");
                std::process::exit(1);
            }
        }
        Command::Open { port } => {
            let hub = yas_webrtc_forwarder::normalize_hub(&cli.connect.hub);
            interactive::run_browser(port, &hub).await;
        }
        Command::Edge => {
            yas_edge::run().await;
        }
        Command::Forward {
            specs,
            all,
            alpn,
            insecure,
        } => {
            // The management verbs share the positional slot with specs: no
            // spec can be a bare word (they all carry colons), so the first
            // argument is unambiguous.
            let verb = specs.first().map(String::as_str).unwrap_or("");
            let rest = specs.get(1..).unwrap_or(&[]);
            let result: Result<i32, String> = match verb {
                "add" => match rest {
                    [name, spec] => forward::cmd_add(name, spec),
                    _ => Err("usage: yas forward add NAME SPEC".into()),
                },
                "list" | "ls" => forward::cmd_list(),
                "rm" | "remove" => match rest {
                    [name] => forward::cmd_rm(name),
                    _ => Err("usage: yas forward rm NAME".into()),
                },
                "toggle" => match rest {
                    [name] => forward::cmd_toggle(name),
                    _ => Err("usage: yas forward toggle NAME".into()),
                },
                _ => match forward::resolve_specs(&specs, all) {
                    // Connect only once there is something to forward, and
                    // only after every spec has parsed.
                    Ok(resolved) => {
                        let conn = &cli.connect;
                        let tls = forward::TlsOpts { alpn, insecure };
                        forward::cmd_forward(conn.on.as_deref(), &conn.hub, resolved, tls).await
                    }
                    Err(e) => Err(e),
                },
            };
            match result {
                Ok(code) => std::process::exit(code),
                Err(e) => {
                    eprintln!("yas: {e}");
                    std::process::exit(1);
                }
            }
        }
        Command::Socks { listen } => {
            // Parse before connecting, so a bad listen address costs nothing.
            let result: Result<i32, String> = match socks::parse_listen(&listen) {
                Ok(listen) => {
                    let conn = &cli.connect;
                    socks::cmd_socks(conn.on.as_deref(), &conn.hub, listen).await
                }
                Err(e) => Err(e),
            };
            match result {
                Ok(code) => std::process::exit(code),
                Err(e) => {
                    eprintln!("yas: {e}");
                    std::process::exit(1);
                }
            }
        }
        Command::Learn => {
            print!("{}", include_str!("learn.md"));
        }
        Command::Generate { output } => {
            generate::run(&output);
        }
        Command::ProxyDaemon => {
            yas_proxy::run(false);
        }
    }
}

/// Read a `usize` limit from the environment. Unset, unparseable or 0 all
/// mean "no limit", which is what the server's `> 0` guards already expect.
fn env_usize(key: &str) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

/// Whether an environment switch is on. Only `1` is on, so a stale `0` or an
/// empty value reads as off rather than as "the variable exists".
fn env_flag(key: &str) -> bool {
    std::env::var(key).is_ok_and(|value| value == "1")
}

/// Open one session against the server in this process, as a pair of boxed
/// halves — what both the edge and the WebRTC share want from an upstream.
fn hosted_connector(endpoint: yas_server::LocalEndpoint) -> yas_webrtc_forwarder::HostedConnector {
    std::sync::Arc::new(move || {
        let endpoint = endpoint.clone();
        Box::pin(async move {
            let stream = endpoint
                .connect()
                .ok_or_else(|| "the server is not accepting sessions".to_owned())?;
            let (reader, writer) = tokio::io::split(stream);
            Ok((
                Box::new(reader) as yas_webrtc_forwarder::BoxedRead,
                Box::new(writer) as yas_webrtc_forwarder::BoxedWrite,
            ))
        })
    })
}

/// Serve the browser from inside the server process.
///
/// Configured exactly like the standalone edge, because it is the same edge:
/// the only difference is where a browser's session comes from, and that a
/// failure here is reported rather than fatal — the server it fronts is still
/// worth running, and the terminals in it are still reachable over the socket.
async fn hosted_edge(endpoint: yas_server::LocalEndpoint) {
    let Some(raw) = yas_edge::passphrase_from_env() else {
        eprintln!(
            "yas server: the hosted edge needs YAS_EDGE_PASSPHRASE or YAS_PASSPHRASE; \
             not serving the browser"
        );
        return;
    };
    let passphrase = match yas_webserver::config::AuthPassphrase::from_env_value(raw) {
        Ok(passphrase) => passphrase,
        Err(error) => {
            eprintln!("yas server: {error}; not serving the browser");
            return;
        }
    };
    let trusted_proxy_ips = match std::env::var("YAS_TRUSTED_PROXY_IPS") {
        Ok(raw) => match yas_edge::parse_trusted_proxy_ips(&raw) {
            Ok(ips) => ips,
            Err(error) => {
                eprintln!("yas server: invalid YAS_TRUSTED_PROXY_IPS: {error}");
                return;
            }
        },
        Err(_) => Default::default(),
    };
    let shutdown = endpoint.shutdown();
    let connector = hosted_connector(endpoint);
    let web_transport = match yas_edge::web_transport_options_from_env() {
        Ok(options) => options,
        Err(error) => {
            eprintln!("yas server: invalid edge WebTransport configuration: {error}");
            return;
        }
    };
    let options = yas_edge::Options {
        passphrase,
        addr: std::env::var("YAS_ADDR").unwrap_or_else(|_| "127.0.0.1:3264".into()),
        home: yas_edge::Home::Hosted(std::sync::Arc::new(move || {
            let connector = connector.clone();
            Box::pin(async move { connector().await })
        })),
        trusted_proxy_ips,
        web_transport,
    };
    if let Err(error) = yas_edge::try_serve(options, Some(shutdown)).await {
        eprintln!("yas server: edge: {error}");
    }
}

fn cmd_hash_passphrase(value: Option<String>) -> Result<(), String> {
    use std::io::Read;

    let passphrase = match value.as_deref() {
        Some("-") | None => {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .map_err(|e| format!("failed to read passphrase from stdin: {e}"))?;
            buf.trim_end_matches(['\r', '\n']).to_string()
        }
        Some(value) => value.to_string(),
    };

    if passphrase.is_empty() {
        return Err("passphrase must be non-empty".to_string());
    }

    let hash = yas_webserver::passphrase::hash(&passphrase)?;
    println!("{hash}");
    Ok(())
}

/// Replace credentials embedded in remote URIs with `****`.
fn mask_remote_credentials(uri: &str) -> String {
    if let Some(rest) = uri.strip_prefix("share:") {
        // Preserve any query string (e.g. ?hub=...).
        return if let Some(q_pos) = rest.find('?') {
            format!("share:****{}", &rest[q_pos..])
        } else {
            "share:****".to_string()
        };
    }
    if let Some(rest) = uri.strip_prefix("uplink:") {
        if let Some((control, _token)) = rest.rsplit_once('#') {
            return format!("uplink:{control}#****");
        }
        // Conservatively redact malformed or obsolete uplink URI forms too.
        return "uplink:****".to_string();
    }
    uri.to_string()
}

/// `yas remote`: administer the target server's Relay catalogue.
///
/// Every verb but `set-default` reaches the server, because that is where the
/// catalogue is (`crates/cli/src/yas_remotes.rs`). `set-default` stays local:
/// which server *this* CLI talks to by default is this machine's business, not
/// something a server should hold an opinion about.
async fn cmd_remote(cmd: RemoteCommand, on: Option<&str>, hub: &str) -> Result<(), String> {
    match cmd {
        RemoteCommand::List { reveal } => {
            let entries = yas_remotes::read(on, hub).await?;
            if entries.is_empty() {
                eprintln!("yas: no remotes configured");
                return Ok(());
            }
            for entry in &entries {
                let uri = if reveal {
                    entry.uri.clone()
                } else {
                    mask_remote_credentials(&entry.uri)
                };
                if entry.disabled {
                    println!("{}\t{}\t(disabled)", entry.name, uri);
                } else {
                    println!("{}\t{}", entry.name, uri);
                }
            }
            Ok(())
        }
        RemoteCommand::Add { name, uri } => {
            // The same rule the document parser enforces. Checking a laxer one
            // here meant `yas remote add 'my remote' ssh:host` printed success,
            // wrote the line, and the next read dropped it.
            if !yas_webserver::config::valid_entry_name(&name) {
                return Err(format!(
                    "invalid remote name '{name}' — no whitespace, '=', or leading '#'"
                ));
            }
            let uri = match uri {
                Some(uri) => uri,
                None => {
                    eprint!("URI for '{name}' (ssh:host, tcp:h:p, socket:/path, local): ");
                    let mut input = String::new();
                    if std::io::stdin().read_line(&mut input).is_err() || input.trim().is_empty() {
                        return Err("no URI provided".to_owned());
                    }
                    input.trim().to_owned()
                }
            };
            let masked = mask_remote_credentials(&uri);
            yas_remotes::modify(on, hub, |entries| {
                match entries.iter_mut().find(|entry| entry.name == name) {
                    Some(entry) => {
                        entry.uri = uri.clone();
                        entry.disabled = false;
                    }
                    None => entries.push(yas_webserver::config::RemoteEntry {
                        name: name.clone(),
                        uri: uri.clone(),
                        disabled: false,
                    }),
                }
                Ok(format!("yas: remote '{name}' set to '{masked}'"))
            })
            .await
        }
        RemoteCommand::Remove { name } => {
            yas_remotes::modify(on, hub, |entries| {
                let before = entries.len();
                entries.retain(|entry| entry.name != name);
                if entries.len() == before {
                    return Err(format!("no remote named '{name}'"));
                }
                Ok(format!("yas: remote '{name}' removed"))
            })
            .await
        }
        RemoteCommand::Toggle { name } => {
            yas_remotes::modify(on, hub, |entries| {
                let Some(entry) = entries.iter_mut().find(|entry| entry.name == name) else {
                    return Err(format!("no remote named '{name}'"));
                };
                entry.disabled = !entry.disabled;
                Ok(if entry.disabled {
                    format!("yas: remote '{name}' disabled")
                } else {
                    format!("yas: remote '{name}' enabled")
                })
            })
            .await
        }
        RemoteCommand::SetDefault { target } => {
            yas_webserver::config::modify_config(|config| {
                if target.is_empty() || target == "local" {
                    config.remove("yas.target");
                } else {
                    config.insert("yas.target".into(), target.clone());
                }
            });
            if target.is_empty() || target == "local" {
                eprintln!("yas: default target cleared (using local)");
            } else {
                eprintln!("yas: default target set to '{target}'");
            }
            Ok(())
        }
    }
}

async fn cmd_install(host: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Reject hosts starting with '-' to prevent SSH option injection.
    let host_check = host.split('@').next_back().unwrap_or(host);
    if host_check.starts_with('-') {
        return Err(format!("invalid ssh host '{host}': must not start with '-'").into());
    }
    let ssh_base = |host: &str| {
        let mut cmd = std::process::Command::new("ssh");
        cmd.arg("-T")
            .arg("-o")
            .arg("ControlMaster=auto")
            .arg("-o")
            .arg("ControlPath=/tmp/yas-ssh-%r@%h:%p")
            .arg("-o")
            .arg("ControlPersist=300")
            .arg(host);
        cmd
    };

    let detect = ssh_base(host)
        .arg("--")
        .arg("uname -s 2>/dev/null || echo WINDOWS")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .output()?;

    if !detect.status.success() {
        return Err("ssh failed to detect remote OS".into());
    }

    let os = String::from_utf8_lossy(&detect.stdout)
        .trim()
        .to_uppercase();

    let install_cmd = if os.contains("WINDOWS")
        || os.contains("MINGW")
        || os.contains("MSYS")
        || os.contains("CYGWIN")
    {
        r#"powershell -ExecutionPolicy Bypass -Command "irm https://yas.run/install.ps1 | iex""#
            .to_string()
    } else {
        r#"sh -c 'if command -v curl >/dev/null 2>&1; then curl -sf https://yas.run | sh; elif command -v wget >/dev/null 2>&1; then wget -qO- https://yas.run | sh; else echo "error: neither curl nor wget found" >&2; exit 1; fi'"#.to_string()
    };

    eprintln!("installing yas on {host} ({os})...");

    let status = ssh_base(host)
        .arg("--")
        .arg(&install_cmd)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()?;

    if !status.success() {
        return Err(format!("remote install exited with {status}").into());
    }

    Ok(())
}

async fn cmd_upgrade() -> Result<(), Box<dyn std::error::Error>> {
    let exe_path = yas_proxy::yas_exe();
    let bin_dir = exe_path
        .parent()
        .ok_or("cannot determine binary directory")?;
    #[cfg(not(windows))]
    let prefix = bin_dir.parent().unwrap_or(bin_dir);

    let install_url = if cfg!(windows) {
        "https://yas.run/install.ps1"
    } else {
        "https://yas.run"
    };
    let script = reqwest::get(install_url)
        .await?
        .error_for_status()?
        .text()
        .await?;

    let ext = if cfg!(windows) { "ps1" } else { "sh" };
    let tmp = std::env::temp_dir().join(format!("yas-install-{}.{}", std::process::id(), ext));
    std::fs::write(&tmp, &script)?;

    #[cfg(unix)]
    {
        let mut cmd = std::process::Command::new("sh");
        cmd.arg(&tmp).env("YAS_PREFIX", prefix);
        // Upgrades keep the flavor: a GPL binary (x264 compiled in) fetches
        // the GPL flavor again.  An explicit YAS_GPL in the environment
        // wins, so users can switch flavors through `yas upgrade`.
        if std::env::var_os("YAS_GPL").is_none() {
            cmd.env(
                "YAS_GPL",
                if cfg!(all(target_os = "linux", feature = "x264")) {
                    "1"
                } else {
                    "0"
                },
            );
        }
        let status = cmd.status()?;
        if status.success() {
            transport::stop_proxy().await;
        }
        std::process::exit(status.code().unwrap_or(1));
    }
    #[cfg(windows)]
    {
        let status = std::process::Command::new("powershell")
            .args(["-ExecutionPolicy", "Bypass", "-File"])
            .arg(&tmp)
            .env("YAS_INSTALL_DIR", bin_dir)
            .status()?;
        if status.success() {
            transport::stop_proxy().await;
        }
        std::process::exit(status.code().unwrap_or(1));
    }
    #[cfg(not(any(unix, windows)))]
    {
        let status = std::process::Command::new("sh")
            .arg(&tmp)
            .env("YAS_PREFIX", prefix)
            .status()?;
        if status.success() {
            transport::stop_proxy().await;
        }
        std::process::exit(status.code().unwrap_or(1));
    }
}

#[cfg(test)]
mod tests {
    use super::{mask_remote_credentials, proxy_daemon_requested};

    #[test]
    fn test_mask_remote_credentials() {
        assert_eq!(mask_remote_credentials("share:mysecret"), "share:****");
        assert_eq!(
            mask_remote_credentials("share:mysecret?hub=yas.run"),
            "share:****?hub=yas.run"
        );
        assert_eq!(
            mask_remote_credentials("uplink:https://relay.example#secret"),
            "uplink:https://relay.example#****"
        );
        assert_eq!(
            mask_remote_credentials("ssh:alice@prod.co"),
            "ssh:alice@prod.co"
        );
        assert_eq!(mask_remote_credentials("local"), "local");
        assert_eq!(mask_remote_credentials("share:"), "share:****");
    }

    #[test]
    fn proxy_daemon_detection_stops_at_extension_command_arguments() {
        assert!(proxy_daemon_requested(["--on", "prod", "proxy-daemon"]));
        assert!(proxy_daemon_requested([
            "--hub=https://hub",
            "proxy-daemon"
        ]));
        assert!(!proxy_daemon_requested([
            "--on",
            "prod",
            "@builder",
            "proxy-daemon"
        ]));
        assert!(!proxy_daemon_requested([
            "--on",
            "proxy-daemon",
            "@builder"
        ]));
    }
}
