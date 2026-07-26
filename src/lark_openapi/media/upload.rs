use std::fmt;

use serde::Deserialize;

use crate::{Error, Result};

use super::super::{
    HttpMethod, MultipartRequest, OpenApiClient, OpenApiMultipartTransport, parse_openapi_response,
};

const IMAGE_PATH: &str = "/open-apis/im/v1/images";
const FILE_PATH: &str = "/open-apis/im/v1/files";

pub const MAX_IMAGE_UPLOAD_BYTES: usize = 10_000_000;
pub const MAX_FILE_UPLOAD_BYTES: usize = 30_000_000;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ImageType {
    #[default]
    Message,
    Avatar,
}

impl ImageType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Message => "message",
            Self::Avatar => "avatar",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    Opus,
    Mp4,
    Pdf,
    Doc,
    Xls,
    Ppt,
    Stream,
}

impl FileType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Opus => "opus",
            Self::Mp4 => "mp4",
            Self::Pdf => "pdf",
            Self::Doc => "doc",
            Self::Xls => "xls",
            Self::Ppt => "ppt",
            Self::Stream => "stream",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ImageCreateRequest {
    pub image_type: ImageType,
    pub image: Vec<u8>,
}

impl fmt::Debug for ImageCreateRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ImageCreateRequest")
            .field("image_type", &self.image_type)
            .field("image_bytes", &self.image.len())
            .finish()
    }
}

impl ImageCreateRequest {
    pub fn message(image: impl Into<Vec<u8>>) -> Self {
        Self {
            image_type: ImageType::Message,
            image: image.into(),
        }
    }

    pub fn avatar(image: impl Into<Vec<u8>>) -> Self {
        Self {
            image_type: ImageType::Avatar,
            image: image.into(),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct FileCreateRequest {
    pub file_type: FileType,
    pub file_name: String,
    pub duration_ms: Option<u64>,
    pub file: Vec<u8>,
}

impl fmt::Debug for FileCreateRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FileCreateRequest")
            .field("file_type", &self.file_type)
            .field("file_name", &self.file_name)
            .field("duration_ms", &self.duration_ms)
            .field("file_bytes", &self.file.len())
            .finish()
    }
}
impl FileCreateRequest {
    pub fn new(
        file_type: FileType,
        file_name: impl Into<String>,
        file: impl Into<Vec<u8>>,
    ) -> Self {
        Self {
            file_type,
            file_name: file_name.into(),
            duration_ms: None,
            file: file.into(),
        }
    }

    pub fn duration_ms(mut self, duration_ms: u64) -> Self {
        self.duration_ms = Some(duration_ms);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageKey(pub String);

impl ImageKey {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileKey(pub String);

impl FileKey {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<T> OpenApiClient<T>
where
    T: OpenApiMultipartTransport,
{
    /// Uploads an image through the official create-image endpoint.
    pub async fn create_image(&self, request: ImageCreateRequest) -> Result<ImageKey> {
        validate_upload_bytes(&request.image, MAX_IMAGE_UPLOAD_BYTES, "image")?;

        let url = self.config().base_url().join(IMAGE_PATH)?;
        let token = self.tenant_access_token().await?;
        let request = MultipartRequest::new(HttpMethod::Post, url)
            .with_bearer_auth(token)
            .text("image_type", request.image_type.as_str())
            .file("image", "image", request.image);
        let response = self.transport().send_multipart(request).await?;
        let response: ImageCreateResponse = parse_openapi_response(response)?;

        Ok(ImageKey(validate_response_key(
            response.data.image_key,
            "image_key",
        )?))
    }

    /// Uploads a file through the official create-file endpoint.
    pub async fn create_file(&self, request: FileCreateRequest) -> Result<FileKey> {
        validate_upload_bytes(&request.file, MAX_FILE_UPLOAD_BYTES, "file")?;
        validate_file_name(&request.file_name)?;

        let url = self.config().base_url().join(FILE_PATH)?;
        let token = self.tenant_access_token().await?;
        let mut multipart = MultipartRequest::new(HttpMethod::Post, url)
            .with_bearer_auth(token)
            .text("file_type", request.file_type.as_str())
            .text("file_name", request.file_name.clone());
        if let Some(duration_ms) = request.duration_ms {
            multipart = multipart.text("duration", duration_ms.to_string());
        }
        let multipart = multipart.file("file", request.file_name, request.file);
        let response = self.transport().send_multipart(multipart).await?;
        let response: FileCreateResponse = parse_openapi_response(response)?;

        Ok(FileKey(validate_response_key(
            response.data.file_key,
            "file_key",
        )?))
    }
}

fn validate_upload_bytes(bytes: &[u8], max_bytes: usize, field: &str) -> Result<()> {
    if bytes.is_empty() {
        return Err(Error::Validation(format!("{field} must not be empty")));
    }
    if bytes.len() > max_bytes {
        return Err(Error::Validation(format!(
            "{field} exceeds the {max_bytes}-byte upload limit"
        )));
    }
    Ok(())
}

fn validate_file_name(file_name: &str) -> Result<()> {
    if file_name.trim().is_empty() {
        return Err(Error::Validation("file_name must not be empty".to_owned()));
    }
    if file_name.chars().any(char::is_control) {
        return Err(Error::Validation(
            "file_name must not contain control characters".to_owned(),
        ));
    }
    if file_name.contains(['/', '\\']) {
        return Err(Error::Validation(
            "file_name must not contain path separators".to_owned(),
        ));
    }
    let Some((stem, extension)) = file_name.rsplit_once('.') else {
        return Err(Error::Validation(
            "file_name must include a suffix".to_owned(),
        ));
    };
    if stem.is_empty() || extension.is_empty() {
        return Err(Error::Validation(
            "file_name must include a non-empty suffix".to_owned(),
        ));
    }
    Ok(())
}

fn validate_response_key(key: String, field: &str) -> Result<String> {
    if key.trim().is_empty() {
        return Err(Error::Transport(format!(
            "OpenAPI response contained an empty {field}"
        )));
    }
    Ok(key)
}

#[derive(Debug, Deserialize)]
struct ImageCreateResponse {
    data: ImageCreateResponseData,
}

#[derive(Debug, Deserialize)]
struct ImageCreateResponseData {
    image_key: String,
}

#[derive(Debug, Deserialize)]
struct FileCreateResponse {
    data: FileCreateResponseData,
}

#[derive(Debug, Deserialize)]
struct FileCreateResponseData {
    file_key: String,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::lark_openapi::test_support::{FakeTransport, block_on};
    use crate::lark_openapi::{HttpResponse, MultipartPart};
    use crate::{ChannelConfig, Error};

    fn token_response() -> HttpResponse {
        HttpResponse::json(
            200,
            json!({
                "code": 0,
                "msg": "ok",
                "tenant_access_token": "tenant-token-1",
                "expire": 7200
            }),
        )
    }

    #[test]
    fn debug_summarizes_upload_request_bytes() {
        let image = ImageCreateRequest::message(vec![1, 2, 3]);
        let file = FileCreateRequest::new(FileType::Mp4, "clip.mp4", vec![4, 5]).duration_ms(1200);

        let debug = format!("{image:?} {file:?}");
        assert!(debug.contains("image_bytes: 3"));
        assert!(debug.contains("file_bytes: 2"));
        assert!(!debug.contains("[1, 2, 3]"));
        assert!(!debug.contains("[4, 5]"));
    }

    #[test]
    fn creates_message_image_with_official_multipart_fields() {
        let transport = FakeTransport::with_multipart_responses(
            vec![token_response()],
            vec![HttpResponse::json(
                200,
                json!({"code": 0, "msg": "ok", "data": {"image_key": "img_123"}}),
            )],
        );
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());

        let key = block_on(client.create_image(ImageCreateRequest::message(vec![1, 2, 3])))
            .expect("image key");

        assert_eq!(key.as_str(), "img_123");
        let calls = transport.multipart_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].method, HttpMethod::Post);
        assert_eq!(
            calls[0].url.as_str(),
            "https://open.feishu.cn/open-apis/im/v1/images"
        );
        assert_eq!(
            calls[0].headers.get("authorization").map(String::as_str),
            Some("Bearer tenant-token-1")
        );
        assert_eq!(
            calls[0].parts,
            vec![
                MultipartPart::Text {
                    name: "image_type".to_owned(),
                    value: "message".to_owned(),
                },
                MultipartPart::File {
                    name: "image".to_owned(),
                    file_name: "image".to_owned(),
                    bytes: vec![1, 2, 3],
                },
            ]
        );
    }

    #[test]
    fn creates_file_with_duration_and_official_multipart_fields() {
        let transport = FakeTransport::with_multipart_responses(
            vec![token_response()],
            vec![HttpResponse::json(
                200,
                json!({"code": 0, "msg": "ok", "data": {"file_key": "file_123"}}),
            )],
        );
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());
        let request =
            FileCreateRequest::new(FileType::Mp4, "clip.mp4", vec![4, 5]).duration_ms(1200);

        let key = block_on(client.create_file(request)).expect("file key");

        assert_eq!(key.as_str(), "file_123");
        assert_eq!(
            transport.multipart_calls()[0].parts,
            vec![
                MultipartPart::Text {
                    name: "file_type".to_owned(),
                    value: "mp4".to_owned(),
                },
                MultipartPart::Text {
                    name: "file_name".to_owned(),
                    value: "clip.mp4".to_owned(),
                },
                MultipartPart::Text {
                    name: "duration".to_owned(),
                    value: "1200".to_owned(),
                },
                MultipartPart::File {
                    name: "file".to_owned(),
                    file_name: "clip.mp4".to_owned(),
                    bytes: vec![4, 5],
                },
            ]
        );
    }

    #[test]
    fn maps_all_file_types_to_official_values() {
        assert_eq!(
            [
                FileType::Opus,
                FileType::Mp4,
                FileType::Pdf,
                FileType::Doc,
                FileType::Xls,
                FileType::Ppt,
                FileType::Stream,
            ]
            .map(FileType::as_str),
            ["opus", "mp4", "pdf", "doc", "xls", "ppt", "stream"]
        );
    }

    #[test]
    fn rejects_invalid_uploads_before_authentication() {
        let transport = FakeTransport::with_multipart_responses(Vec::new(), Vec::new());
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());

        let empty_image = block_on(client.create_image(ImageCreateRequest::message(Vec::new())))
            .expect_err("empty image");
        let empty_name =
            block_on(client.create_file(FileCreateRequest::new(FileType::Stream, "", vec![1])))
                .expect_err("empty filename");
        let control_name = block_on(client.create_file(FileCreateRequest::new(
            FileType::Stream,
            "bad\nname.bin",
            vec![1],
        )))
        .expect_err("control filename");
        let path_name = block_on(client.create_file(FileCreateRequest::new(
            FileType::Stream,
            "../file.bin",
            vec![1],
        )))
        .expect_err("path filename");
        let missing_suffix = block_on(client.create_file(FileCreateRequest::new(
            FileType::Stream,
            "README",
            vec![1],
        )))
        .expect_err("missing suffix");

        assert!(matches!(empty_image, Error::Validation(_)));
        assert!(matches!(empty_name, Error::Validation(_)));
        assert!(matches!(control_name, Error::Validation(_)));
        assert!(matches!(path_name, Error::Validation(_)));
        assert!(matches!(missing_suffix, Error::Validation(_)));
        assert!(transport.calls().is_empty());
        assert!(transport.multipart_calls().is_empty());
    }

    #[test]
    fn rejects_uploads_over_platform_limits_before_authentication() {
        let transport = FakeTransport::with_multipart_responses(Vec::new(), Vec::new());
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());

        let image = block_on(client.create_image(ImageCreateRequest::message(vec![
            0;
            MAX_IMAGE_UPLOAD_BYTES
                + 1
        ])))
        .expect_err("oversized image");
        let file = block_on(client.create_file(FileCreateRequest::new(
            FileType::Stream,
            "large.bin",
            vec![0; MAX_FILE_UPLOAD_BYTES + 1],
        )))
        .expect_err("oversized file");

        assert!(matches!(image, Error::Validation(_)));
        assert!(matches!(file, Error::Validation(_)));
        assert!(transport.calls().is_empty());
    }

    #[test]
    fn preserves_openapi_errors_from_upload_responses() {
        let transport = FakeTransport::with_multipart_responses(
            vec![token_response()],
            vec![HttpResponse::json(
                200,
                json!({"code": 234001, "msg": "Invalid request param"}),
            )],
        );
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport);

        let error = block_on(client.create_image(ImageCreateRequest::message(vec![1])))
            .expect_err("OpenAPI error");

        assert!(matches!(
            error,
            Error::Api {
                code: 234001,
                message
            } if message == "Invalid request param"
        ));
    }

    #[test]
    fn rejects_empty_resource_keys_in_success_responses() {
        let transport = FakeTransport::with_multipart_responses(
            vec![token_response()],
            vec![HttpResponse::json(
                200,
                json!({"code": 0, "msg": "ok", "data": {"image_key": ""}}),
            )],
        );
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport);

        let error = block_on(client.create_image(ImageCreateRequest::message(vec![1])))
            .expect_err("empty image key");

        assert!(matches!(error, Error::Transport(message) if message.contains("empty image_key")));
    }
}
