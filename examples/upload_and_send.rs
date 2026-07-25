use std::env;
use std::io;
use std::path::Path;

use lark_channel::lark_openapi::{OpenApiClient, ReqwestOpenApiTransport};
use lark_channel::{
    ChannelConfig, MediaUpload, MediaUploader, MessageContent, MessageSender, Recipient,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ChannelConfig::new(
        required_env("LARK_APP_ID")?,
        required_env("LARK_APP_SECRET")?,
    );
    let client = OpenApiClient::new(config, ReqwestOpenApiTransport::new());
    let uploader = MediaUploader::new(client.clone());
    let sender = MessageSender::new(client);
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

    let uploaded = uploader.upload(upload_from_env(file_name, bytes)?).await?;
    let content = MessageContent::try_from(uploaded)?;
    let message_id = sender
        .message(recipient_from_env()?, content)
        .send()
        .await?;

    println!("media message sent: {}", message_id.0);

    Ok(())
}

fn upload_from_env(file_name: String, bytes: Vec<u8>) -> Result<MediaUpload, io::Error> {
    match required_env("LARK_UPLOAD_TYPE")?.as_str() {
        "image" => Ok(MediaUpload::image(bytes)),
        "file" => Ok(MediaUpload::file(file_name, bytes)),
        "opus" => Ok(MediaUpload::opus_audio(
            file_name,
            bytes,
            required_duration()?,
        )),
        "mp4" => Ok(MediaUpload::mp4_video(
            file_name,
            bytes,
            required_duration()?,
        )),
        value => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("LARK_UPLOAD_TYPE must be image, file, opus, or mp4; got {value}"),
        )),
    }
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
