use std::env;
use std::io;
use std::path::Path;

use lark_channel::lark_openapi::{OpenApiClient, ReqwestOpenApiTransport};
use lark_channel::{ChannelConfig, MediaUpload, MediaUploader};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ChannelConfig::new(
        required_env("LARK_APP_ID")?,
        required_env("LARK_APP_SECRET")?,
    );
    let client = OpenApiClient::new(config, ReqwestOpenApiTransport::new());
    let uploader = MediaUploader::new(client);
    let path = required_env("LARK_UPLOAD_PATH")?;
    let bytes = std::fs::read(&path)?;
    let file_name = Path::new(&path)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "LARK_UPLOAD_PATH must end in a UTF-8 filename",
            )
        })?
        .to_owned();
    let upload = upload_from_env(file_name, bytes)?;

    let resource = uploader.upload(upload).await?;
    println!(
        "resource uploaded: type={:?}, key={}",
        resource.resource_type(),
        resource.key()
    );

    Ok(())
}

fn upload_from_env(file_name: String, bytes: Vec<u8>) -> Result<MediaUpload, io::Error> {
    match required_env("LARK_UPLOAD_TYPE")?.as_str() {
        "image" => Ok(MediaUpload::image(bytes)),
        "file" => Ok(MediaUpload::file(file_name, bytes)),
        "audio" => Ok(MediaUpload::audio(file_name, bytes, required_duration()?)),
        "video" => Ok(MediaUpload::video(file_name, bytes, required_duration()?)),
        value => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("LARK_UPLOAD_TYPE must be image, file, audio, or video; got {value}"),
        )),
    }
}

fn required_duration() -> Result<u64, io::Error> {
    let value = required_env("LARK_UPLOAD_DURATION_MS")?;
    let duration = value.parse::<u64>().map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("LARK_UPLOAD_DURATION_MS must be a positive integer: {error}"),
        )
    })?;
    if duration == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "LARK_UPLOAD_DURATION_MS must be greater than zero",
        ));
    }
    Ok(duration)
}

fn required_env(name: &str) -> Result<String, io::Error> {
    env::var(name).map_err(|_| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("missing required environment variable: {name}"),
        )
    })
}
