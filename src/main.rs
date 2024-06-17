use std::collections::HashMap;
use std::env;
use std::env::args;
use std::error::Error;
use std::path::PathBuf;
use std::time::Instant;

use http_cache_reqwest::{CACacheManager, Cache, CacheMode, HttpCache, HttpCacheOptions};
use mlua::{Lua, LuaOptions, MultiValue, StdLib};
use reqwest::{Client, Url};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};

struct Meta {
    home: PathBuf,
    base: Url,
    tool: Url,
    fragment: String,
    args: Vec<String>,
    env: HashMap<String, String>,
}

struct YasRT {
    pub meta: Meta,
    pub lua: Lua,
    pub http_client: ClientWithMiddleware,
}

impl YasRT {
    async fn new(args: Vec<String>, env: HashMap<String, String>) -> Result<Self, Box<dyn Error>> {
        let base = Url::parse(env.get("YAS_BASE").unwrap_or(&"https://oh.yas.tools".to_string()))?;
        let tool_ref = if args.len() > 0 { args[0].as_str() } else { "repl" };
        let args = args[1..].to_vec();
        let (tool, fragment) = urlize(base.clone(), tool_ref)?;
        let local = dirs::data_local_dir().ok_or("no local data dir")?;
        let home = local.join("yas.tools");
        let manager = CACacheManager { path: home.join("http-cache") };
        let cache = HttpCache {
            mode: CacheMode::Default,
            manager,
            options: HttpCacheOptions::default(),
        };
        let meta = Meta {
            base,
            home,
            tool,
            fragment,
            args,
            env,
        };
        Ok(Self {
            meta,
            lua: Lua::new_with(StdLib::ALL_SAFE, LuaOptions::default())?,
            http_client: ClientBuilder::new(Client::new()).with(Cache(cache)).build(),
        })
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let init = Instant::now();
    let rt = YasRT::new(args().skip(1).collect(), env::vars().collect()).await?;
    let setup = Instant::now();
    print!("fetching {}\n", rt.meta.tool);
    let body = rt.http_client
        .get(rt.meta.tool.as_str())
        .send().await?
        .text().await?;
    let fetched = Instant::now();
    let v = rt.lua
        .load(body.as_str())
        .eval::<MultiValue>()?;
    let evaled = Instant::now();
    println!(
        "setup: {:?}, fetch: {:?}, eval: {:?}",
        setup - init,
        fetched - setup,
        evaled - fetched
    );
    println!("{:?}", v);
    Ok(())
}

fn urlize(root: Url, input: &str) -> Result<(Url, String), Box<dyn Error>> {
    root.join(input).map(|it| (it, "".to_string())).map_err(Into::into)
}
