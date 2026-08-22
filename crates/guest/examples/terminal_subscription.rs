//! Minimal native Terminal query from an extension guest.

use yas_guest::{Client, terminal};

fn extension(mut client: Client) -> Result<(), terminal::Error> {
    let handle = client
        .context()
        .argv
        .first()
        .and_then(|argument| core::str::from_utf8(argument).ok())
        .and_then(|argument| argument.parse::<u64>().ok())
        .unwrap_or(1);
    let generation = client
        .context()
        .argv
        .get(1)
        .and_then(|argument| core::str::from_utf8(argument).ok())
        .and_then(|argument| argument.parse::<u32>().ok())
        .unwrap_or(1);
    let _ = client.query_terminal_cwd(handle, generation, terminal::DEFAULT_QUERY_WINDOW)?;
    Ok(())
}

yas_guest::entry!(extension);

fn main() {}
