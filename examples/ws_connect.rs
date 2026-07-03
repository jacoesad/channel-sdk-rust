use std::env;
use std::error::Error;
use std::io;

use lark_channel::lark_openapi::{
    OpenApiClient, ReqwestOpenApiTransport, TokioTungsteniteWebSocketTransport, WebSocketEventAck,
};
use lark_channel::{ChannelConfig, ChannelEvent, EventConsumer};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = ChannelConfig::new(
        required_env("LARK_APP_ID")?,
        required_env("LARK_APP_SECRET")?,
    );
    let openapi = OpenApiClient::new(config, ReqwestOpenApiTransport::new());

    let endpoint = openapi.websocket_endpoint().await?;
    println!(
        "websocket endpoint acquired: {}",
        redacted_endpoint_url(endpoint.url())
    );
    println!("device_id: {:?}", endpoint.device_id());
    println!("service_id: {:?}", endpoint.service_id());
    println!("client_config: {:?}", endpoint.client_config());

    if env::var("LARK_WS_CONNECT").ok().as_deref() == Some("1") {
        let transport = TokioTungsteniteWebSocketTransport::new();
        let connection = transport.connect(&endpoint).await?;
        println!(
            "websocket connected: device_id={:?}, service_id={:?}",
            connection.device_id(),
            connection.service_id()
        );
        let mut consumer = EventConsumer::new(connection);
        if env::var("LARK_WS_RECEIVE_ONCE").ok().as_deref() == Some("1") {
            println!("waiting for one websocket event");
            let handled = consumer
                .handle_next_event(|event| async move {
                    print_received_event(&event);
                    Ok(WebSocketEventAck::ok())
                })
                .await?;
            if handled {
                println!("websocket event acknowledged");
            }
        }
        consumer.into_inner().close().await?;
        println!("websocket closed");
    }

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

fn redacted_endpoint_url(url: &url::Url) -> String {
    match url.host_str() {
        Some(host) => format!("{}://{}{}", url.scheme(), host, url.path()),
        None => format!("{}:{}", url.scheme(), url.path()),
    }
}
