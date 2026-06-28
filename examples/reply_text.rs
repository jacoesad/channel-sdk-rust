use std::env;
use std::io;

use lark_channel::{ChannelConfig, MessageId, OpenApiClient, ReqwestOpenApiTransport};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ChannelConfig::new(
        required_env("LARK_APP_ID")?,
        required_env("LARK_APP_SECRET")?,
    );
    let client = OpenApiClient::new(config, ReqwestOpenApiTransport::new());

    let parent_message_id = MessageId(required_env("LARK_MESSAGE_ID")?);
    let text = env::var("LARK_TEXT").unwrap_or_else(|_| "reply from lark-channel".to_owned());
    let message_id = client.reply_text_message(parent_message_id, text).await?;

    println!("message replied: {}", message_id.0);

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
