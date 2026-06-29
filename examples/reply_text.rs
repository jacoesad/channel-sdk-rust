use std::env;
use std::io;

use lark_channel::lark_openapi::{MessageReplyOptions, OpenApiClient, ReqwestOpenApiTransport};
use lark_channel::{ChannelConfig, MessageId};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ChannelConfig::new(
        required_env("LARK_APP_ID")?,
        required_env("LARK_APP_SECRET")?,
    );
    let client = OpenApiClient::new(config, ReqwestOpenApiTransport::new());

    let parent_message_id = MessageId(required_env("LARK_MESSAGE_ID")?);
    let text = env::var("LARK_TEXT").unwrap_or_else(|_| "reply from lark-channel".to_owned());
    let options = reply_options_from_env()?;
    let message_id = client
        .reply_text_message_with_options(parent_message_id, text, options)
        .await?;

    println!("message replied: {}", message_id.0);

    Ok(())
}

fn reply_options_from_env() -> Result<MessageReplyOptions, io::Error> {
    let mut options = MessageReplyOptions::new();
    if let Ok(uuid) = env::var("LARK_UUID") {
        options = options.uuid(uuid);
    }
    if let Ok(reply_in_thread) = env::var("LARK_REPLY_IN_THREAD") {
        options = options.reply_in_thread(parse_bool("LARK_REPLY_IN_THREAD", &reply_in_thread)?);
    }
    Ok(options)
}

fn parse_bool(name: &str, value: &str) -> Result<bool, io::Error> {
    match value {
        "true" | "1" | "yes" => Ok(true),
        "false" | "0" | "no" => Ok(false),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{name} must be one of: true, false, 1, 0, yes, no"),
        )),
    }
}

fn required_env(name: &str) -> Result<String, io::Error> {
    env::var(name).map_err(|_| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("missing required environment variable: {name}"),
        )
    })
}
