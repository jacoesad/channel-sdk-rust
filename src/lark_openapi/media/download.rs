use crate::media::DownloadedResource;
use crate::validation::validate_path_identifier;
use crate::{Error, Result};

use super::super::{HttpMethod, HttpRequest, OpenApiBinaryTransport, OpenApiClient};

pub const MAX_MESSAGE_RESOURCE_BYTES: usize = 100_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageResourceType {
    Image,
    File,
}

impl MessageResourceType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::File => "file",
        }
    }
}

impl<T> OpenApiClient<T>
where
    T: OpenApiBinaryTransport,
{
    /// Downloads a resource attached to a received message.
    ///
    /// Lark/Feishu accepts `image` for image messages and rich-text images,
    /// and `file` for files, audio, and video. Sticker resources are not
    /// supported by the official endpoint.
    pub async fn get_message_resource(
        &self,
        message_id: &str,
        file_key: &str,
        resource_type: MessageResourceType,
    ) -> Result<DownloadedResource> {
        validate_path_identifier(message_id, "message_id")?;
        validate_path_identifier(file_key, "file_key")?;

        let path = format!("/open-apis/im/v1/messages/{message_id}/resources/{file_key}");
        let mut url = self.config().base_url().join(&path)?;
        url.query_pairs_mut()
            .append_pair("type", resource_type.as_str());

        let token = self.tenant_access_token().await?;
        let request = HttpRequest::empty(HttpMethod::Get, url).with_bearer_auth(token);
        let response = self
            .transport()
            .send_bytes(request, MAX_MESSAGE_RESOURCE_BYTES)
            .await?;

        if !(200..300).contains(&response.status) {
            return Err(Error::HttpStatus {
                status: response.status,
            });
        }

        Ok(DownloadedResource {
            content_type: response.header("content-type").map(ToOwned::to_owned),
            content_disposition: response
                .header("content-disposition")
                .map(ToOwned::to_owned),
            bytes: response.body,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;
    use crate::lark_openapi::test_support::{FakeTransport, block_on};
    use crate::lark_openapi::{BinaryHttpResponse, HttpResponse};
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
    fn uses_the_platform_message_resource_limit() {
        assert_eq!(MAX_MESSAGE_RESOURCE_BYTES, 100_000_000);
    }

    #[test]
    fn downloads_message_image_with_binary_metadata() {
        let headers = BTreeMap::from([
            ("Content-Type".to_owned(), "image/png".to_owned()),
            (
                "Content-Disposition".to_owned(),
                "attachment; filename=\"image.png\"".to_owned(),
            ),
        ]);
        let transport = FakeTransport::with_binary_responses(
            vec![token_response()],
            vec![BinaryHttpResponse::new(
                200,
                headers,
                vec![0, 159, 146, 150],
            )],
        );
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());

        let resource =
            block_on(client.get_message_resource("om_123", "img_456", MessageResourceType::Image))
                .expect("downloaded resource");

        assert_eq!(resource.bytes, vec![0, 159, 146, 150]);
        assert_eq!(resource.content_type.as_deref(), Some("image/png"));
        assert_eq!(
            resource.content_disposition.as_deref(),
            Some("attachment; filename=\"image.png\"")
        );

        let calls = transport.calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[1].method, HttpMethod::Get);
        assert_eq!(
            calls[1].url.as_str(),
            "https://open.feishu.cn/open-apis/im/v1/messages/om_123/resources/img_456?type=image"
        );
        assert_eq!(
            calls[1].headers.get("authorization").map(String::as_str),
            Some("Bearer tenant-token-1")
        );
        assert_eq!(calls[1].body, serde_json::Value::Null);
        assert_eq!(
            calls[1].max_response_bytes,
            Some(MAX_MESSAGE_RESOURCE_BYTES)
        );
    }

    #[test]
    fn maps_file_resources_to_the_official_query_value() {
        let transport = FakeTransport::with_binary_responses(
            vec![token_response()],
            vec![BinaryHttpResponse::new(
                200,
                BTreeMap::new(),
                b"file".to_vec(),
            )],
        );
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());

        block_on(client.get_message_resource("om_123", "file_456", MessageResourceType::File))
            .expect("downloaded resource");

        assert_eq!(
            transport.calls()[1].url.as_str(),
            "https://open.feishu.cn/open-apis/im/v1/messages/om_123/resources/file_456?type=file"
        );
    }

    #[test]
    fn rejects_invalid_resource_identifiers_before_authentication() {
        let transport = FakeTransport::with_binary_responses(Vec::new(), Vec::new());
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());

        let error = block_on(client.get_message_resource(
            "om_123/other",
            "file_456",
            MessageResourceType::File,
        ))
        .expect_err("invalid path identifier");

        assert!(matches!(error, Error::Validation(_)));
        assert!(transport.calls().is_empty());
    }

    #[test]
    fn preserves_non_success_http_status() {
        let transport = FakeTransport::with_binary_responses(
            vec![token_response()],
            vec![BinaryHttpResponse::new(
                400,
                BTreeMap::from([("content-type".to_owned(), "application/json".to_owned())]),
                br#"{"code":234003,"msg":"File not in message"}"#.to_vec(),
            )],
        );
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport);

        let error =
            block_on(client.get_message_resource("om_123", "file_456", MessageResourceType::File))
                .expect_err("HTTP error");

        assert!(matches!(error, Error::HttpStatus { status: 400 }));
    }
}
