use std::env;
use std::error::Error;
use std::io;
use std::time::Instant;

use lark_channel::ChannelConfig;
use lark_channel::lark_openapi::{
    OpenApiClient, ReqwestOpenApiTransport, TokioTungsteniteWebSocketTransport, WebSocketEventAck,
};

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
        let mut connection = transport.connect(&endpoint).await?;
        println!(
            "websocket connected: device_id={:?}, service_id={:?}",
            connection.device_id(),
            connection.service_id()
        );
        if env::var("LARK_WS_RECEIVE_ONCE").ok().as_deref() == Some("1") {
            println!("waiting for one websocket event");
            if let Some((frame, event)) = connection.next_event().await? {
                let started = Instant::now();
                println!(
                    "websocket event received: message_id={:?}, trace_id={:?}, payload_len={}",
                    event.message_id(),
                    event.trace_id(),
                    event.payload().len()
                );
                let biz_rt = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
                connection
                    .ack_event(&frame, WebSocketEventAck::ok().with_biz_rt(biz_rt))
                    .await?;
                println!("websocket event acknowledged");
            }
        }
        connection.close().await?;
        println!("websocket closed");
    }

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

fn redacted_endpoint_url(url: &url::Url) -> String {
    match url.host_str() {
        Some(host) => format!("{}://{}{}", url.scheme(), host, url.path()),
        None => format!("{}:{}", url.scheme(), url.path()),
    }
}
