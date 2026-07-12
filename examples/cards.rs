use std::env;
use std::io;

use lark_channel::lark_openapi::{CardUpdateOptions, OpenApiClient, ReqwestOpenApiTransport};
use lark_channel::{Card, CardElement, ChannelConfig, MessageSender, Recipient};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ChannelConfig::new(
        required_env("LARK_APP_ID")?,
        required_env("LARK_APP_SECRET")?,
    );
    let openapi = OpenApiClient::new(config, ReqwestOpenApiTransport::new());
    let sender = MessageSender::new(openapi);
    let recipient = recipient_from_env()?;

    let card = example_card("CardKit 2.0 message", "Created by `lark-channel`")?;
    if env_flag("LARK_CARD_ENTITY")? {
        send_card_entity(&sender, recipient, card).await?;
    } else {
        send_inline_card(&sender, recipient, card).await?;
    }

    Ok(())
}

async fn send_inline_card(
    sender: &MessageSender<ReqwestOpenApiTransport>,
    recipient: Recipient,
    card: Card,
) -> Result<(), Box<dyn std::error::Error>> {
    let message_id = sender.card_message(recipient, card).send().await?;
    println!("inline card sent: {}", message_id.0);

    if env_flag("LARK_UPDATE_CARD")? {
        let updated = example_card("CardKit 2.0 message", "Updated by `message_id`")?;
        sender
            .client()
            .update_message_card(&message_id, &updated)
            .await?;
        println!("inline card updated: {}", message_id.0);
    }

    Ok(())
}

async fn send_card_entity(
    sender: &MessageSender<ReqwestOpenApiTransport>,
    recipient: Recipient,
    card: Card,
) -> Result<(), Box<dyn std::error::Error>> {
    let card_id = sender.client().create_card_entity(&card).await?;
    let message_id = sender
        .card_reference_message(recipient, card_id.clone())
        .send()
        .await?;
    println!(
        "card entity sent: card_id={}, message_id={}",
        card_id.as_str(),
        message_id.0
    );

    if env_flag("LARK_UPDATE_CARD")? {
        let updated = example_card("CardKit 2.0 entity", "Updated by `card_id`")?;
        let sequence = env::var("LARK_CARD_SEQUENCE")
            .unwrap_or_else(|_| "1".to_owned())
            .parse::<u32>()
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "LARK_CARD_SEQUENCE must be a positive integer",
                )
            })?;
        let mut options = CardUpdateOptions::new(sequence);
        if let Ok(uuid) = env::var("LARK_CARD_UPDATE_UUID") {
            options = options.uuid(uuid);
        }
        sender
            .client()
            .update_card_entity(&card_id, &updated, options)
            .await?;
        println!("card entity updated: {}", card_id.as_str());
    }

    Ok(())
}

fn example_card(title: &str, content: &str) -> lark_channel::Result<Card> {
    let docs = CardElement::open_url_button("Open documentation", "https://open.feishu.cn")?;
    let callback = CardElement::callback_button("Acknowledge", json!({ "action": "ack" }))?
        .element_id("ack_button")?;

    Card::builder()
        .header(title)
        .header_template("blue")
        .markdown(content)
        .divider()
        .element(docs)
        .element(callback)
        .build()
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

fn env_flag(name: &str) -> Result<bool, io::Error> {
    match env::var(name) {
        Ok(value) => value.parse::<bool>().map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{name} must be true or false"),
            )
        }),
        Err(env::VarError::NotPresent) => Ok(false),
        Err(error) => Err(io::Error::new(io::ErrorKind::InvalidData, error)),
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
