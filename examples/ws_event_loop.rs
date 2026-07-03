use std::env;
use std::error::Error;
use std::io;
use std::time::Duration;

use lark_channel::lark_openapi::{
    OpenApiClient, ReqwestOpenApiTransport, TokioTungsteniteWebSocketTransport, WebSocketEventAck,
};
use lark_channel::{
    ChannelConfig, ChannelEvent, EventLoop, EventLoopOptions, OpenApiWebSocketEventConnector,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = ChannelConfig::new(
        required_env("LARK_APP_ID")?,
        required_env("LARK_APP_SECRET")?,
    );
    let openapi = OpenApiClient::new(config, ReqwestOpenApiTransport::new());
    let connector =
        OpenApiWebSocketEventConnector::new(openapi, TokioTungsteniteWebSocketTransport::new());
    let options = EventLoopOptions::new()
        .with_max_reconnects(optional_usize("LARK_WS_MAX_RECONNECTS")?.unwrap_or(3))
        .with_reconnect_delay(Duration::from_millis(
            optional_u64("LARK_WS_RECONNECT_DELAY_MS")?.unwrap_or(1000),
        ));

    println!(
        "starting websocket event loop: max_reconnects={}, reconnect_delay={:?}",
        options.max_reconnects(),
        options.reconnect_delay()
    );

    let mut event_loop = EventLoop::with_options(connector, options);
    let exit = event_loop
        .run(|event| async move {
            print_received_event(&event);
            Ok(WebSocketEventAck::ok())
        })
        .await?;

    println!("websocket event loop exited: {exit:?}");
    Ok(())
}

fn print_received_event(event: &lark_channel::ReceivedEvent) {
    println!(
        "websocket event received: message_id={:?}, trace_id={:?}, payload_len={}",
        event.message_id, event.trace_id, event.payload_len
    );
    match &event.event {
        ChannelEvent::Message(message) => {
            println!(
                "message event parsed: chat_id={}, chat_type={:?}, sender={}, text={:?}, mentions={}",
                message.chat_id,
                message.chat_type,
                message.sender.open_id,
                message.text,
                message.mentions.len()
            );
        }
        ChannelEvent::Unknown { context, .. } => {
            println!("event parsed as unknown: context={context:?}");
        }
        ChannelEvent::CardAction { context, .. } => {
            println!("card action event parsed: context={context:?}");
        }
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
