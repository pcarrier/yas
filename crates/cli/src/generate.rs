use crate::cli;
use clap::{Arg, Command, CommandFactory};
use clap_complete::Shell;
use std::fs;
use std::path::Path;

const BASH_RUNTIME_COMPLETION: &str = r#"

# Advertised extension commands are discovered only during explicit completion.
_yas_with_extension_commands() {
    _yas "$@"
    local _yas_current="${COMP_WORDS[COMP_CWORD]}"
    local -a _yas_previous=()
    local _yas_index _yas_candidate
    for ((_yas_index = 1; _yas_index < COMP_CWORD; _yas_index++)); do
        _yas_previous+=("${COMP_WORDS[_yas_index]}")
    done
    while IFS= read -r _yas_candidate; do
        if [[ -n "$_yas_candidate" ]]; then
            COMPREPLY+=("$_yas_candidate")
        fi
    done < <(command "${COMP_WORDS[0]}" __complete-extension \
        "--current=$_yas_current" -- "${_yas_previous[@]}" 2>/dev/null)
}
complete -F _yas_with_extension_commands -o bashdefault -o default yas
"#;

const ZSH_RUNTIME_COMPLETION: &str = r#"

# Advertised extension commands are discovered only during explicit completion.
functions[_yas_static]=$functions[_yas]
_yas() {
    local -a _yas_original_words _yas_previous _yas_dynamic
    local _yas_original_current _yas_index _yas_static_result
    _yas_original_words=("${words[@]}")
    _yas_original_current=$CURRENT
    _yas_static "$@"
    _yas_static_result=$?
    for ((_yas_index = 2; _yas_index < _yas_original_current; _yas_index++)); do
        _yas_previous+=("${_yas_original_words[_yas_index]}")
    done
    _yas_dynamic=("${(@f)$(command "${_yas_original_words[1]}" \
        __complete-extension \
        "--current=${_yas_original_words[_yas_original_current]}" -- \
        "${_yas_previous[@]}" 2>/dev/null)}")
    if (( ${#_yas_dynamic} )); then
        compadd -- "${_yas_dynamic[@]}"
        return 0
    fi
    return $_yas_static_result
}
"#;

const FISH_RUNTIME_COMPLETION: &str = r#"

# Advertised extension commands are discovered only during explicit completion.
function __fish_yas_extension_commands
    set -l _yas_words (commandline -opc)
    set -l _yas_executable $_yas_words[1]
    set -e _yas_words[1]
    command $_yas_executable __complete-extension \
        "--current="(commandline -ct) -- $_yas_words 2>/dev/null
end
complete -c yas -f -a '(__fish_yas_extension_commands)'
"#;

/// Build a clap Command for the standalone YAS edge (mirrors its env-var config).
fn yas_edge_cmd(name: &'static str) -> Command {
    Command::new(name)
        .version(env!("CARGO_PKG_VERSION"))
        .about("YAS browser edge over WebSocket")
        .long_about(
            "yas-edge serves the browser UI, authenticates browser transports, and \
             adapts one authenticated yas.v1 WebSocket to one fixed home YAS socket. It does \
             not parse protocol messages or hold route credentials.\n\n\
             Use it for always-on deployments behind a TLS reverse proxy or as a systemd \
             service. For local and SSH use, the yas(1) CLI embeds equivalent edge \
             functionality and is simpler to run.\n\n\
             All configuration is via environment variables.",
        )
        .after_help(
            "ENVIRONMENT:\n    \
             YAS_PASSPHRASE    Browser passphrase, or argon2 PHC hash from yas hash-passphrase (required)\n    \
             YAS_ADDR          Listen address (default: 127.0.0.1:3264)\n    \
             YAS_SOCK           Fixed home YAS server socket\n    \
             YAS_SERVER_UID     Required numeric home-server peer UID (default: edge euid)\n    \
             YAS_TRUSTED_PROXY_IPS Exact reverse-proxy IPs allowed to supply X-Forwarded-For",
        )
}

/// Build a clap Command for yas-webrtc-forwarder.
fn yas_webrtc_forwarder_cmd() -> Command {
    Command::new("yas-webrtc-forwarder")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Forward a yas server terminal over WebRTC")
        .long_about(
            "yas-webrtc-forwarder connects to a yas server Unix socket and \
             bridges it to browsers over WebRTC data channels. It handles signaling, \
             STUN/TURN NAT traversal, and peer-to-peer connections.\n\n\
             For most use cases, yas share is simpler -- it runs the forwarder \
             in-process and auto-starts a server if needed. The standalone binary is \
             for custom deployments where the server is managed separately.",
        )
        .arg(
            Arg::new("socket")
                .long("socket")
                .value_name("PATH")
                .env("YAS_SOCK")
                .required(true)
                .help("Path to the yas server Unix socket"),
        )
        .arg(
            Arg::new("passphrase")
                .long("passphrase")
                .value_name("PASSPHRASE")
                .env("YAS_PASSPHRASE")
                .required(true)
                .help("Share passphrase"),
        )
        .arg(
            Arg::new("hub")
                .long("hub")
                .value_name("URL")
                .env("YAS_HUB")
                .default_value("https://yas.run")
                .help("Signaling hub URL"),
        )
        .arg(
            Arg::new("message")
                .long("message")
                .value_name("TEMPLATE")
                .help("Override the message template (use {secret} as placeholder)"),
        )
        .arg(
            Arg::new("quiet")
                .long("quiet")
                .action(clap::ArgAction::SetTrue)
                .help("Don't print the sharing URL"),
        )
        .arg(
            Arg::new("verbose")
                .long("verbose")
                .action(clap::ArgAction::SetTrue)
                .help("Print detailed connection diagnostics to stderr"),
        )
}

fn generate_man_page(cmd: Command, out_dir: &Path) {
    let name = cmd.get_name().to_string();
    let man = clap_mangen::Man::new(cmd);
    let mut buf = Vec::new();
    man.render(&mut buf).expect("failed to render man page");
    let path = out_dir.join(format!("{name}.1"));
    fs::write(&path, buf).unwrap_or_else(|e| panic!("failed to write {}: {e}", path.display()));
}

fn generate_completions(mut cmd: Command, out_dir: &Path, name: &str) {
    for shell in [Shell::Fish, Shell::Bash, Shell::Zsh] {
        let dir = match shell {
            Shell::Fish => out_dir.join("fish/vendor_completions.d"),
            Shell::Bash => out_dir.join("bash-completion/completions"),
            Shell::Zsh => out_dir.join("zsh/site-functions"),
            _ => unreachable!(),
        };
        fs::create_dir_all(&dir).unwrap();
        let path = clap_complete::generate_to(shell, &mut cmd, name, &dir).unwrap();
        let hook = match shell {
            Shell::Bash => BASH_RUNTIME_COMPLETION,
            Shell::Zsh => ZSH_RUNTIME_COMPLETION,
            Shell::Fish => FISH_RUNTIME_COMPLETION,
            _ => unreachable!(),
        };
        let mut generated = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        generated.push_str(hook);
        fs::write(&path, generated)
            .unwrap_or_else(|error| panic!("failed to extend {}: {error}", path.display()));
    }
}

pub fn run(output: &str) {
    let base = Path::new(output);

    // Man pages
    let man_dir = base.join("man/man1");
    fs::create_dir_all(&man_dir).unwrap();

    clap_mangen::generate_to(cli::Cli::command(), &man_dir).expect("failed to generate man pages");
    generate_man_page(yas_edge_cmd("yas-edge"), &man_dir);
    generate_man_page(yas_webrtc_forwarder_cmd(), &man_dir);

    // Shell completions (for the main yas CLI only)
    generate_completions(cli::Cli::command(), base, "yas");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn generated_shells_call_the_hidden_runtime_query() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let output = std::env::temp_dir().join(format!(
            "yas-completion-test-{}-{unique}",
            std::process::id()
        ));
        generate_completions(cli::Cli::command(), &output, "yas");

        let bash = fs::read_to_string(output.join("bash-completion/completions/yas.bash")).unwrap();
        let zsh = fs::read_to_string(output.join("zsh/site-functions/_yas")).unwrap();
        let fish = fs::read_to_string(output.join("fish/vendor_completions.d/yas.fish")).unwrap();
        assert!(bash.contains("_yas_with_extension_commands"));
        assert!(bash.contains("__complete-extension"));
        assert!(zsh.contains("functions[_yas_static]=$functions[_yas]"));
        assert!(zsh.contains("__complete-extension"));
        assert!(fish.contains("__fish_yas_extension_commands"));
        assert!(fish.contains("__complete-extension"));

        fs::remove_dir_all(output).unwrap();
    }

    #[test]
    fn hidden_query_is_absent_from_normal_root_help() {
        let help = cli::Cli::command().render_long_help().to_string();
        assert!(!help.contains("__complete-extension"));
    }

    #[test]
    fn edge_man_command_is_standalone() {
        let edge = yas_edge_cmd("yas-edge").render_long_help().to_string();
        assert!(edge.contains("yas-edge serves the browser UI"));
        assert!(edge.contains("one fixed home YAS socket"));
        assert!(!edge.contains("destination mux"));
    }
}
