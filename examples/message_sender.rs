use std::env;
use std::io;

use lark_channel::lark_openapi::{OpenApiClient, ReqwestOpenApiTransport};
use lark_channel::{ChannelConfig, MessageSender, MessageSenderOptions, Recipient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ChannelConfig::new(
        required_env("LARK_APP_ID")?,
        required_env("LARK_APP_SECRET")?,
    );
    let openapi = OpenApiClient::new(config, ReqwestOpenApiTransport::new());
    let sender = MessageSender::with_options(openapi, sender_options_from_env()?);

    let recipient = recipient_from_env()?;
    let text = env::var("LARK_TEXT").unwrap_or_else(|_| "hello from lark-channel".to_owned());
    let mut operation = sender.text_message(recipient, text);
    if let Ok(uuid) = env::var("LARK_UUID") {
        operation = operation.uuid(uuid);
    }
    let message_id = operation.send().await?;

    println!("message sent: {}", message_id.0);

    Ok(())
}

fn sender_options_from_env() -> Result<MessageSenderOptions, io::Error> {
    let mut options = MessageSenderOptions::new();
    if let Ok(max_attempts) = env::var("LARK_MAX_ATTEMPTS") {
        options.set_max_attempts(parse_usize("LARK_MAX_ATTEMPTS", &max_attempts)?);
    }
    Ok(options)
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

fn parse_usize(name: &str, value: &str) -> Result<usize, io::Error> {
    value.parse().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{name} must be a positive integer"),
        )
    })
}

fn required_env(name: &str) -> Result<String, io::Error> {
    env::var(name).map_err(|_| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("missing required environment variable: {name}"),
        )
    })
}
