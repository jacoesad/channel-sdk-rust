# Media

`lark-channel` can upload in-memory media, send or reply with uploaded resource keys, and download resource files attached to received Lark/Feishu messages. The high-level `MediaUploader`, `MessageSender`, and `MediaDownloader` map Channel resource types to the official media OpenAPI endpoints while leaving filesystem and URL policy to the application.

## Supported Resources

| Descriptor type | Resource key | OpenAPI type |
| --- | --- | --- |
| `Image` | `image_key` | `image` |
| `File` | `file_key` | `file` |
| `Audio` | `file_key` | `file` |
| `Media` (video) | `file_key` | `file` |

A media descriptor can contain an optional cover `image_key`; `MediaDownloader::download` returns the primary video `file_key` resource. Use `OpenApiClient::get_message_resource` directly with `MessageResourceType::Image` when the cover is needed.

Folder, sticker, unknown, and incomplete descriptors fail validation before authentication. The official endpoint does not support sticker resources or merged-forward child-message resources, and the bot must belong to the message conversation.

## Upload

`MediaUploader` accepts owned bytes so applications remain responsible for choosing and securing their input source:

```rust
use lark_channel::lark_openapi::{OpenApiClient, ReqwestOpenApiTransport};
use lark_channel::{ChannelConfig, MediaUpload, MediaUploader};

let client = OpenApiClient::new(
    ChannelConfig::new(app_id, app_secret),
    ReqwestOpenApiTransport::new(),
);
let uploader = MediaUploader::new(client);
let uploaded = uploader
    .upload(MediaUpload::file("report.pdf", report_bytes))
    .await?;

println!("uploaded key: {}", uploaded.key());
```

The high-level mappings are:

| Upload kind | OpenAPI endpoint | Official type | Required metadata | Limit |
| --- | --- | --- | --- | --- |
| Image | create image | `message` | bytes | 10 MB |
| File | create file | `stream` | filename, bytes | 30 MB |
| OPUS audio | create file | `opus` | `.opus` filename, pre-encoded bytes, positive duration | 30 MB |
| MP4 video | create file | `mp4` | `.mp4` filename, pre-encoded bytes, positive duration | 30 MB |

The application must enable either `im:resource` or `im:resource:upload` for these upload endpoints.

`UploadedResource::Image` contains an `image_key`. `UploadedResource::File` contains a `file_key`, filename, resource type, and optional duration. `MediaUploader` performs one request and does not automatically retry because the upload endpoints do not accept an idempotency key.

Use `OpenApiClient::create_image` or `OpenApiClient::create_file` when the application needs the complete low-level official type set, including avatar images and `pdf`, `doc`, `xls`, or `ppt` file types.

## Send Uploaded Resources

Uploading and sending are deliberately separate operations. Convert an `UploadedResource` into `MessageContent`, then send it through the existing message builder:

```rust
use lark_channel::{
    MediaUpload, MediaUploader, MessageContent, MessageSender, Recipient,
};

let uploader = MediaUploader::new(client.clone());
let sender = MessageSender::new(client);
let uploaded = uploader.upload(MediaUpload::image(image_bytes)).await?;
let content = MessageContent::try_from(uploaded)?;

let message_id = sender
    .message(Recipient::Chat(chat_id), content)
    .send()
    .await?;
```

The conversion maps uploaded images to `image`, generic files to `file`, OPUS audio to `audio`, and MP4 video to the official `media` message type. It does not attach a video cover automatically; use `MessageContent::Media` or `MessageSender::media_message` when an uploaded cover `image_key` is available.

This sequence performs two OpenAPI requests. Uploads are not retried automatically because those endpoints have no idempotency key. The subsequent message send uses the normal `MessageSender` UUID and transport-retry behavior. If sending fails after a successful upload, the resource remains uploaded and the application decides whether to retry the send with the same logical UUID.

## Download

```rust
use lark_channel::lark_openapi::{OpenApiClient, ReqwestOpenApiTransport};
use lark_channel::{ChannelConfig, MediaDownloader, ResourceDescriptor};

let client = OpenApiClient::new(
    ChannelConfig::new(app_id, app_secret),
    ReqwestOpenApiTransport::new(),
);
let downloader = MediaDownloader::new(client);

// `descriptor` normally comes from `NormalizedMessage::resources`.
let resource = downloader.download(&descriptor).await?;
println!("downloaded {} bytes", resource.len());
```

`DownloadedResource` contains the complete in-memory bytes and optional `Content-Type` and `Content-Disposition` response metadata. The Reqwest transport enforces the platform's 100 MB per-resource limit while reading the response. Callers should release large byte buffers promptly.

The public `ChannelClient::download_resource` contract follows the same
memory-only boundary and returns `DownloadedResource`; applications decide
whether and where to persist those bytes.

### Migrating from v0.5

In v0.6, `ChannelClient::download_resource` changed from
`download_resource(resource, path) -> Result<()>` to
`download_resource(resource) -> Result<DownloadedResource>`. Implementations
must remove the destination path parameter and return the downloaded bytes
together with optional response metadata. Applications now own filesystem
policy and persist the returned bytes when needed.

The SDK does not read or write local paths, fetch arbitrary URLs, transcode media, infer audio/video duration, or retry media transfers. Applications can read an approved path and pass the resulting bytes to `MediaUploader`. Audio must already be OPUS, and video must already be MP4; the high-level uploader checks the filename suffix while the platform validates the actual media bytes. A future path or URL source adapter must define explicit filesystem and SSRF policy before it is added.
