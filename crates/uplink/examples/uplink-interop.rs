//! Native endpoint for the browser WebCrypto interoperability test.
//! Only disposable test keys belong in this process's environment.
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let identity = yas_uplink::Identity::from_base64(&std::env::var("YAS_UPLINK_IDENTITY")?)?;
    let allowed = std::env::var("YAS_UPLINK_TEST_CLIENT")?.parse()?;
    let carrier = tokio::io::join(tokio::io::stdin(), tokio::io::stdout());
    let mut stream = yas_uplink::accept(carrier, identity.server_config(vec![allowed])?).await?;
    let mut bytes = [0; 4096];
    loop {
        let count = stream.read(&mut bytes).await?;
        if count == 0 {
            break;
        }
        stream.write_all(&bytes[..count]).await?;
        stream.flush().await?;
    }
    stream.shutdown().await?;
    Ok(())
}
