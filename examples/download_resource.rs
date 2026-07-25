use std::env;
use std::io;

use lark_channel::lark_openapi::{OpenApiClient, ReqwestOpenApiTransport};
use lark_channel::{ChannelConfig, MediaDownloader, ResourceDescriptor, ResourceType};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ChannelConfig::new(
        required_env("LARK_APP_ID")?,
        required_env("LARK_APP_SECRET")?,
    );
    let client = OpenApiClient::new(config, ReqwestOpenApiTransport::new());
    let downloader = MediaDownloader::new(client);
    let resource_type = resource_type_from_env()?;
    let resource_key = required_env("LARK_RESOURCE_KEY")?;
    let descriptor = ResourceDescriptor {
        message_id: required_env("LARK_MESSAGE_ID")?,
        resource_type,
        file_key: (resource_type != ResourceType::Image).then_some(resource_key.clone()),
        image_key: (resource_type == ResourceType::Image).then_some(resource_key),
        file_name: None,
        duration_ms: None,
    };

    let resource = downloader.download(&descriptor).await?;
    println!(
        "resource downloaded: {} bytes, content_type={}, content_disposition={}",
        resource.len(),
        resource.content_type.as_deref().unwrap_or("unknown"),
        resource.content_disposition.as_deref().unwrap_or("none")
    );

    Ok(())
}

fn resource_type_from_env() -> Result<ResourceType, io::Error> {
    match required_env("LARK_RESOURCE_TYPE")?.as_str() {
        "image" => Ok(ResourceType::Image),
        "file" => Ok(ResourceType::File),
        value => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("LARK_RESOURCE_TYPE must be image or file, got {value}"),
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
