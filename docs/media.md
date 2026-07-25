# Media

`lark-channel` can download resource files attached to received Lark/Feishu messages. Message normalization exposes resource keys as `ResourceDescriptor` values, and `MediaDownloader` maps supported descriptors to the official message-resource API.

## Supported Resources

| Descriptor type | Resource key | OpenAPI type |
| --- | --- | --- |
| `Image` | `image_key` | `image` |
| `File` | `file_key` | `file` |
| `Audio` | `file_key` | `file` |
| `Media` (video) | `file_key` | `file` |

A media descriptor can contain an optional cover `image_key`; `MediaDownloader::download` returns the primary video `file_key` resource. Use `OpenApiClient::get_message_resource` directly with `MessageResourceType::Image` when the cover is needed.

Folder, sticker, unknown, and incomplete descriptors fail validation before authentication. The official endpoint does not support sticker resources or merged-forward child-message resources, and the bot must belong to the message conversation.

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

This milestone stage does not write files, choose local paths, fetch arbitrary URLs, retry downloads, or upload images/files. Those behaviors remain separate so applications retain control over storage and security policy.
