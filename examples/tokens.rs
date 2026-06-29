use std::env;
use std::io;

use lark_channel::ChannelConfig;
use lark_channel::lark_openapi::{OpenApiClient, ReqwestOpenApiTransport};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ChannelConfig::new(
        required_env("LARK_APP_ID")?,
        required_env("LARK_APP_SECRET")?,
    );
    let client = OpenApiClient::new(config, ReqwestOpenApiTransport::new());

    let app_token = client.app_access_token().await?;
    let tenant_token = client.tenant_access_token().await?;

    println!("app access token acquired: {} bytes", app_token.len());
    println!("tenant access token acquired: {} bytes", tenant_token.len());

    Ok(())
}

fn required_env(name: &str) -> Result<String, io::Error> {
    env::var(name).map_err(|_| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("missing required environment variable: {name}"),
        )
    })
}
