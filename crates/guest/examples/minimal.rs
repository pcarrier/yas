use yas_guest::{Client, Error};

fn extension(mut client: Client) -> Result<(), Error> {
    let _ = client.ping()?;
    Ok(())
}

yas_guest::entry!(extension);

// Cargo examples are binaries. Wasmi invokes only the separately exported
// `yas_main`; this empty Rust binary entry is not exported from the module.
fn main() {}
