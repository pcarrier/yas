use yas_guest::command::{CommandProvider, Error, LogLevel, ProviderEvent};

const DESCRIPTOR: &str = r#"{
  "protocol":"yas.cli.v1",
  "summary":"Small typed SDK example",
  "commands":[{"path":[],"summary":"Report the argument count"}]
}"#;

fn extension(mut client: yas_guest::Client) -> Result<(), Error> {
    let listener_name = format!(
        "yas.cli.{:016x}.{}",
        client.context().extension_handle,
        client.context().attempt
    );
    let listener = client.listen_channel(&listener_name, b"")?;
    let mut provider = CommandProvider::register(&mut client, listener, DESCRIPTOR)?;

    loop {
        match provider.accept(&mut client)? {
            ProviderEvent::Invocation(mut invocation) => {
                let count = invocation.request().args.len();
                invocation.log(&mut client, LogLevel::Info, "handling command")?;
                invocation.stdout(&mut client, format!("{count} argument(s)\n").as_bytes())?;
                invocation.result(&mut client, "application/json", br#"{"ok":true}"#)?;
                invocation.exit(&mut client, 0, "")?;
            }
            ProviderEvent::Closed(_) => return Ok(()),
        }
    }
}

yas_guest::entry!(extension);

// Cargo examples are binaries; Wasmi calls the exported `yas_main`.
fn main() {}
