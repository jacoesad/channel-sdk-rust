use std::env;
use std::io;

use lark_channel::lark_openapi::{MessageSendOptions, OpenApiClient, ReqwestOpenApiTransport};
use lark_channel::{ChannelConfig, Recipient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ChannelConfig::new(
        required_env("LARK_APP_ID")?,
        required_env("LARK_APP_SECRET")?,
    );
    let client = OpenApiClient::new(config, ReqwestOpenApiTransport::new());

    let recipient = recipient_from_env()?;
    let text = env::var("LARK_TEXT").unwrap_or_else(|_| "hello from lark-channel".to_owned());
    let options = send_options_from_env();
    let message_id = client
        .send_text_message_with_options(recipient, text, options)
        .await?;

    println!("message sent: {}", message_id.0);

    Ok(())
}

fn send_options_from_env() -> MessageSendOptions {
    let mut options = MessageSendOptions::new();
    if let Ok(uuid) = env::var("LARK_UUID") {
        options = options.uuid(uuid);
    }
    options
}

fn recipient_from_env() -> Result<Recipient, io::Error> {
    if let Ok(chat_id) = env::var("LARK_CHAT_ID") {
        return Ok(Recipient::Chat(chat_id));
    }
    if let Ok(open_id) = env::var("LARK_OPEN_ID") {
        return Ok(Recipient::User(open_id));
    }

    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "missing required environment variable: LARK_CHAT_ID or LARK_OPEN_ID",
    ))
}

fn required_env(name: &str) -> Result<String, io::Error> {
    env::var(name).map_err(|_| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("missing required environment variable: {name}"),
        )
    })
}
