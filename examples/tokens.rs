use std::env;

use lark_channel::{ChannelConfig, OpenApiClient, ReqwestOpenApiTransport};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ChannelConfig::new(env::var("LARK_APP_ID")?, env::var("LARK_APP_SECRET")?);
    let client = OpenApiClient::new(config, ReqwestOpenApiTransport::new());

    let app_token = client.app_access_token().await?;
    let tenant_token = client.tenant_access_token().await?;

    println!("app access token acquired: {} bytes", app_token.len());
    println!("tenant access token acquired: {} bytes", tenant_token.len());

    Ok(())
}
