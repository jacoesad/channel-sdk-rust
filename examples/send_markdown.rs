use std::env;
use std::io;

use lark_channel::lark_openapi::{OpenApiClient, ReqwestOpenApiTransport};
use lark_channel::{ChannelConfig, MessageSender, PostContent, Recipient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ChannelConfig::new(
        required_env("LARK_APP_ID")?,
        required_env("LARK_APP_SECRET")?,
    );
    let openapi = OpenApiClient::new(config, ReqwestOpenApiTransport::new());
    let sender = MessageSender::new(openapi);

    let recipient = recipient_from_env()?;
    let markdown = env::var("LARK_MARKDOWN").unwrap_or_else(|_| {
        "## Hello from lark-channel\n\n- Native **Markdown**\n- [Lark OpenAPI](https://open.feishu.cn)"
            .to_owned()
    });
    let post = match env::var("LARK_TITLE") {
        Ok(title) => PostContent::builder()
            .title(title)
            .markdown(markdown)
            .build()?,
        Err(_) => PostContent::markdown(markdown),
    };

    let mut operation = sender.post_message(recipient, post);
    if let Ok(uuid) = env::var("LARK_UUID") {
        operation = operation.uuid(uuid);
    }
    let message_id = operation.send().await?;

    println!("markdown message sent: {}", message_id.0);

    Ok(())
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
