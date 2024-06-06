use std::time::Instant;

use mimalloc::MiMalloc;
use reqwest::Client;
use rquickjs::{AsyncContext, AsyncRuntime, Module};
use sqlite::Value;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

struct YasRT {
    pub ctx: AsyncContext,
    pub conn: sqlite::Connection,
    pub http_client: Client,
}

impl YasRT {
    async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let rt = AsyncRuntime::new()?;
        let ctx = AsyncContext::full(&rt).await?;
        let conn = sqlite::Connection::open(":memory:")?;
        let client = Client::new();
        Ok(Self { ctx, conn, http_client: client })
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let init = Instant::now();
    let rt = YasRT::new().await?;
    let setup = Instant::now();
    let body = rt.http_client.get("https://oh.yas.tools/3")
        .send().await?.text().await?;
    let fetched = Instant::now();
    let mut bound = rt.conn.prepare("SELECT ?")?
        .into_iter()
        .bind((1, body.as_str()))?;
    bound.next();
    let queried: Instant;
    let evaled: Instant;
    match bound.read(0).unwrap() {
        Value::String(body) => {
            queried = Instant::now();
            let v = rt.ctx.with(|ctx| {
                Module::evaluate(ctx, "test", body.as_bytes())?.finish::<()>()
            }).await?;
            evaled = Instant::now();
            println!("{:?}", v)
        }
        v => panic!("unexpected value {v:?}"),
    }
    println!("setup: {:?}, fetch: {:?}, query: {:?}, eval: {:?}",
             setup - init, fetched - setup, queried - fetched, evaled - queried);
    Ok(())
}
