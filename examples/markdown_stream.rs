use std::env;
use std::io;
use std::time::Duration;

use lark_channel::lark_openapi::{OpenApiClient, ReqwestOpenApiTransport};
use lark_channel::{
    ChannelConfig, Error, MarkdownStream, MarkdownStreamBuilder, MessageId, MessageSender,
    Recipient, ThrottledMarkdownStream,
};

const DEFAULT_TEXT: &str =
    "## Streaming reply\n\nThis content is arriving through the high-level Markdown stream.";
const DEFAULT_CHUNK_CHARS: usize = 12;
const DEFAULT_INTERVAL_MS: u64 = 150;
const DEFAULT_CHUNK_DELAY_MS: u64 = 25;
const MIN_INTERVAL_MS: u64 = 100;
const DEFAULT_MAX_ATTEMPTS: usize = 3;

#[derive(Debug, PartialEq, Eq)]
enum StreamTarget {
    Message(Recipient),
    Reply(MessageId),
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ChannelConfig::new(
        required_env("LARK_APP_ID")?,
        required_env("LARK_APP_SECRET")?,
    );
    let openapi = OpenApiClient::new(config, ReqwestOpenApiTransport::new());
    let sender = MessageSender::new(openapi);

    let target = resolve_target(
        optional_env("LARK_MESSAGE_ID")?,
        optional_env("LARK_CHAT_ID")?,
        optional_env("LARK_OPEN_ID")?,
    )?;
    let is_reply = matches!(target, StreamTarget::Reply(_));
    let mut builder = match target {
        StreamTarget::Reply(message_id) => sender.markdown_stream_reply(message_id),
        StreamTarget::Message(recipient) => sender.markdown_stream_message(recipient),
    };

    if is_reply {
        if let Some(reply_in_thread) = optional_bool("LARK_REPLY_IN_THREAD")? {
            builder = builder.reply_in_thread(reply_in_thread);
        }
    }
    if let Some(uuid) = optional_env("LARK_UUID")? {
        builder = builder.uuid(uuid);
    }
    let max_attempts = optional_usize("LARK_MAX_ATTEMPTS")?
        .unwrap_or(DEFAULT_MAX_ATTEMPTS)
        .max(1);
    builder = builder.max_attempts(1);

    let content = validate_stream_text(
        env::var("LARK_STREAM_TEXT").unwrap_or_else(|_| DEFAULT_TEXT.to_owned()),
    )?;
    builder.preflight_content(&content)?;
    let chunk_chars = optional_usize("LARK_STREAM_CHUNK_CHARS")?
        .unwrap_or(DEFAULT_CHUNK_CHARS)
        .max(1);
    let interval_ms = optional_u64("LARK_STREAM_INTERVAL_MS")?.unwrap_or(DEFAULT_INTERVAL_MS);
    if interval_ms < MIN_INTERVAL_MS {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("LARK_STREAM_INTERVAL_MS must be at least {MIN_INTERVAL_MS}"),
        )
        .into());
    }

    let update_interval = Duration::from_millis(interval_ms);
    let chunk_delay = Duration::from_millis(
        optional_u64("LARK_STREAM_CHUNK_DELAY_MS")?.unwrap_or(DEFAULT_CHUNK_DELAY_MS),
    );
    let stream = match start_with_retry(&mut builder, max_attempts, update_interval).await {
        Ok(stream) => stream,
        Err(error) => {
            if let Some(card_id) = builder.prepared_card_id() {
                eprintln!(
                    "stream delivery failed after card creation: card_id={}",
                    card_id.as_str()
                );
            }
            return Err(error.into());
        }
    };
    let mut stream = stream.throttle(update_interval);
    let chunks = chunk_text(&content, chunk_chars);
    for chunk in &chunks {
        if let Err(error) =
            append_with_retry(&mut stream, chunk, max_attempts, update_interval).await
        {
            report_stream_failure(&stream);
            return Err(error.into());
        }
        tokio::time::sleep(chunk_delay).await;
    }
    if let Err(error) = finish_with_retry(&mut stream, max_attempts, update_interval).await {
        report_stream_failure(&stream);
        return Err(error.into());
    }

    println!(
        "markdown stream completed: card_id={}, message_id={}",
        stream.card_id().as_str(),
        stream.message_id().0
    );
    Ok(())
}

async fn start_with_retry<'a>(
    builder: &mut MarkdownStreamBuilder<'a, ReqwestOpenApiTransport>,
    max_attempts: usize,
    interval: Duration,
) -> lark_channel::Result<MarkdownStream<'a, ReqwestOpenApiTransport>> {
    let mut attempt = 0;
    loop {
        attempt += 1;
        match builder.start().await {
            Ok(stream) => return Ok(stream),
            Err(Error::Transport(_))
                if attempt < max_attempts && builder.prepared_card_id().is_some() =>
            {
                tokio::time::sleep(interval).await;
            }
            Err(error) => return Err(error),
        }
    }
}

async fn append_with_retry(
    stream: &mut ThrottledMarkdownStream<'_, ReqwestOpenApiTransport>,
    chunk: &str,
    max_attempts: usize,
    interval: Duration,
) -> lark_channel::Result<()> {
    let mut attempt = 0;
    loop {
        attempt += 1;
        let result = if attempt == 1 {
            stream.append(chunk).await
        } else {
            stream.flush().await
        };
        match result {
            Ok(()) => return Ok(()),
            Err(Error::Transport(_)) if attempt < max_attempts => {
                tokio::time::sleep(interval).await;
            }
            Err(error) => return Err(error),
        }
    }
}

async fn finish_with_retry(
    stream: &mut ThrottledMarkdownStream<'_, ReqwestOpenApiTransport>,
    max_attempts: usize,
    interval: Duration,
) -> lark_channel::Result<()> {
    let mut attempt = 0;
    loop {
        attempt += 1;
        match stream.finish().await {
            Ok(()) => return Ok(()),
            Err(Error::Transport(_)) if attempt < max_attempts => {
                tokio::time::sleep(interval).await;
            }
            Err(error) => return Err(error),
        }
    }
}

fn report_stream_failure(stream: &ThrottledMarkdownStream<'_, ReqwestOpenApiTransport>) {
    eprintln!(
        "stream update failed: card_id={}, message_id={}, pending={}, buffered={}",
        stream.card_id().as_str(),
        stream.message_id().0,
        stream.has_pending_operation(),
        stream.has_buffered_content()
    );
}

fn chunk_text(text: &str, chunk_chars: usize) -> Vec<String> {
    let characters = text.chars().collect::<Vec<_>>();
    characters
        .chunks(chunk_chars.max(1))
        .map(|chunk| chunk.iter().collect())
        .collect()
}

fn validate_stream_text(text: String) -> Result<String, io::Error> {
    if text.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "LARK_STREAM_TEXT must not be empty",
        ));
    }
    Ok(text)
}

fn resolve_target(
    message_id: Option<String>,
    chat_id: Option<String>,
    open_id: Option<String>,
) -> Result<StreamTarget, io::Error> {
    if let Some(message_id) = message_id {
        return Ok(StreamTarget::Reply(MessageId(message_id)));
    }
    if let Some(chat_id) = chat_id {
        return Ok(StreamTarget::Message(Recipient::Chat(chat_id)));
    }
    if let Some(open_id) = open_id {
        return Ok(StreamTarget::Message(Recipient::User(open_id)));
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "missing required environment variable: LARK_MESSAGE_ID, LARK_CHAT_ID, or LARK_OPEN_ID",
    ))
}

fn required_env(name: &str) -> Result<String, io::Error> {
    optional_env(name)?.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("missing required environment variable: {name}"),
        )
    })
}

fn optional_env(name: &str) -> Result<Option<String>, io::Error> {
    match env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("environment variable is not valid Unicode: {name}"),
        )),
    }
}

fn optional_bool(name: &str) -> Result<Option<bool>, io::Error> {
    optional_env(name)?
        .map(|value| match value.as_str() {
            "true" | "1" => Ok(true),
            "false" | "0" => Ok(false),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{name} must be true, false, 1, or 0"),
            )),
        })
        .transpose()
}

fn optional_usize(name: &str) -> Result<Option<usize>, io::Error> {
    optional_env(name)?
        .map(|value| {
            value.parse::<usize>().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("{name} must be a non-negative integer"),
                )
            })
        })
        .transpose()
}

fn optional_u64(name: &str) -> Result<Option<u64>, io::Error> {
    optional_env(name)?
        .map(|value| {
            value.parse::<u64>().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("{name} must be a non-negative integer"),
                )
            })
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_text_on_unicode_character_boundaries() {
        assert_eq!(
            chunk_text("ab你好cd", 2),
            vec!["ab".to_owned(), "你好".to_owned(), "cd".to_owned()]
        );
    }

    #[test]
    fn rejects_empty_stream_text() {
        assert!(validate_stream_text(String::new()).is_err());
    }

    #[test]
    fn resolves_targets_in_documented_precedence_order() {
        assert_eq!(
            resolve_target(
                Some("om_1".to_owned()),
                Some("oc_1".to_owned()),
                Some("ou_1".to_owned()),
            )
            .expect("reply target"),
            StreamTarget::Reply(MessageId("om_1".to_owned()))
        );
        assert_eq!(
            resolve_target(None, Some("oc_1".to_owned()), Some("ou_1".to_owned()))
                .expect("chat target"),
            StreamTarget::Message(Recipient::Chat("oc_1".to_owned()))
        );
        assert_eq!(
            resolve_target(None, None, Some("ou_1".to_owned())).expect("user target"),
            StreamTarget::Message(Recipient::User("ou_1".to_owned()))
        );
    }
}
