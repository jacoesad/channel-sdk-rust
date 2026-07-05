use std::env;
use std::io;
use std::time::Duration;

use lark_channel::lark_openapi::{
    OpenApiClient, OpenApiTransport, ReqwestOpenApiTransport, TokioTungsteniteWebSocketTransport,
    WebSocketEventAck,
};
use lark_channel::{
    ChannelConfig, ChannelEvent, EventLoop, EventLoopOptions, MessageChatType, MessageId,
    MessageSender, MessageSenderOptions, MessageSenderType, NormalizedMessage,
    OpenApiWebSocketEventConnector, ReceivedEvent,
};

const OPENAPI_UUID_MAX_CHARS: usize = 50;
const ECHO_UUID_PREFIX: &str = "echo-";
const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x00000100000001b3;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ChannelConfig::new(
        required_env("LARK_APP_ID")?,
        required_env("LARK_APP_SECRET")?,
    );
    let openapi = OpenApiClient::new(config, ReqwestOpenApiTransport::new());
    let sender = MessageSender::with_options(openapi.clone(), sender_options_from_env()?);
    let echo = EchoConfig::from_env()?;
    let connector =
        OpenApiWebSocketEventConnector::new(openapi, TokioTungsteniteWebSocketTransport::new());
    let mut options = EventLoopOptions::new()
        .with_max_reconnects(optional_usize("LARK_WS_MAX_RECONNECTS")?.unwrap_or(3))
        .with_reconnect_delay(Duration::from_millis(
            optional_u64("LARK_WS_RECONNECT_DELAY_MS")?.unwrap_or(1000),
        ));
    if optional_bool("LARK_WS_USE_SERVER_RECONNECT_CONFIG")?.unwrap_or(false) {
        options = options.with_server_reconnect_config(true);
    }
    if let Some(timeout_ms) = optional_u64("LARK_WS_HEARTBEAT_TIMEOUT_MS")? {
        options = options.with_heartbeat_timeout(Some(Duration::from_millis(timeout_ms)));
    }

    println!(
        "starting echo bot: max_reconnects={}, reconnect_delay={:?}, server_reconnect_config={}, heartbeat_timeout={:?}",
        options.max_reconnects(),
        options.reconnect_delay(),
        options.use_server_reconnect_config(),
        options.heartbeat_timeout()
    );

    let mut event_loop = EventLoop::with_options(connector, options);
    let exit = event_loop
        .run(move |event| {
            let sender = sender.clone();
            let echo = echo.clone();
            async move { handle_echo_event(sender, echo, event).await }
        })
        .await?;

    println!("echo bot event loop exited: {exit:?}");
    Ok(())
}

async fn handle_echo_event<T>(
    sender: MessageSender<T>,
    echo: EchoConfig,
    event: ReceivedEvent,
) -> lark_channel::Result<WebSocketEventAck>
where
    T: OpenApiTransport,
{
    let ChannelEvent::Message(message) = event.event else {
        println!(
            "skipping non-message event: message_id={:?}",
            event.message_id
        );
        return Ok(WebSocketEventAck::ok());
    };

    if message.sender.sender_type == MessageSenderType::Bot {
        println!("skipping bot-authored message: {}", message.message_id);
        return Ok(WebSocketEventAck::ok());
    }
    if !should_echo_message(&message, &echo) {
        println!(
            "skipping message outside echo policy: message_id={}, chat_type={:?}",
            message.message_id, message.chat_type
        );
        return Ok(WebSocketEventAck::ok());
    }

    let text = message.text.trim();
    if text.is_empty() {
        println!("skipping empty text message: {}", message.message_id);
        return Ok(WebSocketEventAck::ok());
    }

    let reply_text = format!("{}{}", echo.prefix, text);
    let reply_id = sender
        .text_reply(MessageId(message.message_id.clone()), reply_text)
        .uuid(echo_uuid_for_message_id(&message.message_id))
        .reply_in_thread(echo.reply_in_thread)
        .send()
        .await?;
    println!(
        "echo reply sent: parent_message_id={}, reply_message_id={}",
        message.message_id, reply_id.0
    );

    Ok(WebSocketEventAck::ok())
}

fn echo_uuid_for_message_id(message_id: &str) -> String {
    let prefixed_len = ECHO_UUID_PREFIX.chars().count() + message_id.chars().count();
    if prefixed_len <= OPENAPI_UUID_MAX_CHARS {
        return format!("{ECHO_UUID_PREFIX}{message_id}");
    }

    format!("{ECHO_UUID_PREFIX}{:016x}", stable_hash(message_id))
}

fn stable_hash(value: &str) -> u64 {
    value.bytes().fold(FNV_OFFSET_BASIS, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME)
    })
}

fn should_echo_message(message: &NormalizedMessage, echo: &EchoConfig) -> bool {
    match message.chat_type {
        MessageChatType::P2p => true,
        MessageChatType::Group => {
            echo.echo_all_group_messages
                || echo
                    .bot_open_id
                    .as_deref()
                    .is_some_and(|bot_open_id| message.mentions_bot(bot_open_id))
        }
        MessageChatType::Unknown => false,
    }
}

#[derive(Debug, Clone)]
struct EchoConfig {
    bot_open_id: Option<String>,
    echo_all_group_messages: bool,
    prefix: String,
    reply_in_thread: bool,
}

impl EchoConfig {
    fn from_env() -> Result<Self, io::Error> {
        Ok(Self {
            bot_open_id: env::var("LARK_BOT_OPEN_ID").ok(),
            echo_all_group_messages: optional_bool("LARK_ECHO_ALL_GROUP_MESSAGES")?
                .unwrap_or(false),
            prefix: env::var("LARK_ECHO_PREFIX").unwrap_or_else(|_| "echo: ".to_owned()),
            reply_in_thread: optional_bool("LARK_ECHO_REPLY_IN_THREAD")?.unwrap_or(false),
        })
    }
}

fn sender_options_from_env() -> Result<MessageSenderOptions, io::Error> {
    let mut options = MessageSenderOptions::new();
    if let Ok(max_attempts) = env::var("LARK_MAX_ATTEMPTS") {
        options.set_max_attempts(parse_usize("LARK_MAX_ATTEMPTS", &max_attempts)?);
    }
    Ok(options)
}

fn required_env(name: &str) -> Result<String, io::Error> {
    env::var(name).map_err(|_| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("missing required environment variable: {name}"),
        )
    })
}

fn optional_usize(name: &str) -> Result<Option<usize>, io::Error> {
    env::var(name)
        .ok()
        .map(|value| parse_usize(name, &value))
        .transpose()
}

fn optional_u64(name: &str) -> Result<Option<u64>, io::Error> {
    env::var(name)
        .ok()
        .map(|value| parse_u64(name, &value))
        .transpose()
}

fn optional_bool(name: &str) -> Result<Option<bool>, io::Error> {
    env::var(name)
        .ok()
        .map(|value| parse_bool(name, &value))
        .transpose()
}

fn parse_usize(name: &str, value: &str) -> Result<usize, io::Error> {
    value.parse().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{name} must be a non-negative integer"),
        )
    })
}

fn parse_u64(name: &str, value: &str) -> Result<u64, io::Error> {
    value.parse().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{name} must be a non-negative integer"),
        )
    })
}

fn parse_bool(name: &str, value: &str) -> Result<bool, io::Error> {
    match value {
        "1" | "true" | "TRUE" | "True" | "yes" | "YES" | "Yes" => Ok(true),
        "0" | "false" | "FALSE" | "False" | "no" | "NO" | "No" => Ok(false),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{name} must be true or false"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echo_uuid_uses_short_message_id_directly() {
        assert_eq!(echo_uuid_for_message_id("om_123"), "echo-om_123");
    }

    #[test]
    fn echo_uuid_hashes_long_message_id_within_openapi_limit() {
        let message_id = format!("om_{}", "x".repeat(80));

        let uuid = echo_uuid_for_message_id(&message_id);

        assert_eq!(uuid, echo_uuid_for_message_id(&message_id));
        assert!(uuid.starts_with(ECHO_UUID_PREFIX));
        assert!(uuid.chars().count() <= OPENAPI_UUID_MAX_CHARS);
    }
}
