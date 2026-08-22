//! Protocol-neutral values and parsers shared by the native Terminal CLI.

/// Default cap on a single terminal output read.
pub(crate) const OUTPUT_MAX_BYTES: u32 = 256 * 1024;

/// Default number of records `yas terminal journal` prints.
pub(crate) const JOURNAL_LIMIT: u16 = 20;

pub(crate) struct StartRequest {
    pub(crate) tag: Option<String>,
    pub(crate) command: Vec<String>,
    pub(crate) shell: bool,
    pub(crate) cwd: Option<String>,
    pub(crate) env: Vec<String>,
    pub(crate) rows: u16,
    pub(crate) cols: u16,
    pub(crate) deadline: Option<u64>,
}

/// Split a `KEY=VALUE` pair the way `env(1)` does: on the first `=`.
pub(crate) fn parse_env_assignment(entry: &str) -> Result<(&str, &str), String> {
    match entry.split_once('=') {
        Some(("", _)) => Err(format!("--env needs a name before the '=': {entry:?}")),
        Some(pair) => Ok(pair),
        None => Err(format!("--env needs KEY=VALUE, got {entry:?}")),
    }
}

pub(crate) fn parse_escapes(input: &str) -> Vec<u8> {
    let mut output = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' && index + 1 < bytes.len() {
            match bytes[index + 1] {
                // Real terminals send CR for Enter. A literal LF remains
                // available as `\\x0a`.
                b'n' | b'r' => {
                    output.push(b'\r');
                    index += 2;
                }
                b't' => {
                    output.push(b'\t');
                    index += 2;
                }
                b'\\' => {
                    output.push(b'\\');
                    index += 2;
                }
                b'0' => {
                    output.push(0);
                    index += 2;
                }
                b'x' if index + 3 < bytes.len() => {
                    if let (Some(high), Some(low)) =
                        (hex_digit(bytes[index + 2]), hex_digit(bytes[index + 3]))
                    {
                        output.push(high << 4 | low);
                        index += 4;
                    } else {
                        output.push(bytes[index]);
                        index += 1;
                    }
                }
                _ => {
                    output.push(bytes[index]);
                    index += 1;
                }
            }
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    output
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_assignment_splits_only_once() {
        assert_eq!(parse_env_assignment("FOO=bar"), Ok(("FOO", "bar")));
        assert_eq!(
            parse_env_assignment("URL=http://x/?a=b"),
            Ok(("URL", "http://x/?a=b"))
        );
        assert!(parse_env_assignment("FOO").is_err());
        assert!(parse_env_assignment("=bar").is_err());
    }

    #[test]
    fn terminal_escapes_match_keyboard_bytes() {
        assert_eq!(parse_escapes("hello\\n"), b"hello\r");
        assert_eq!(parse_escapes("\\t\\\\\\0"), &[b'\t', b'\\', 0]);
        assert_eq!(parse_escapes("\\x1b[A"), &[0x1b, b'[', b'A']);
        assert_eq!(parse_escapes("\\x0a"), b"\n");
        assert_eq!(parse_escapes("\\xzz"), b"\\xzz");
    }
}
