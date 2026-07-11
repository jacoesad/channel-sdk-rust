use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::card::{Card, CardId};
use crate::message::MessageId;
use crate::{Error, Result};

use super::message::validate_message_id;
use super::{OpenApiClient, OpenApiTransport};

const CARD_ENTITY_PATH: &str = "/open-apis/cardkit/v1/cards";
const MESSAGE_PATH: &str = "/open-apis/im/v1/messages";
const MAX_CARD_UPDATE_UUID_CHARS: usize = 64;
const MAX_CARD_UPDATE_SEQUENCE: u32 = i32::MAX as u32;

impl<T> OpenApiClient<T>
where
    T: OpenApiTransport,
{
    /// Replaces the content of a sent interactive message by `message_id`.
    ///
    /// The card must explicitly set `config.update_multi=true`, as required by
    /// the official message-card update API.
    pub async fn update_message_card(&self, message_id: &MessageId, card: &Card) -> Result<()> {
        validate_message_id(message_id)?;
        card.validate_for_message_update()?;
        let path = format!("{MESSAGE_PATH}/{}", message_id.0);
        let request = UpdateMessageCardRequest {
            content: serde_json::to_string(card)?,
        };
        let _: Value = self.patch_tenant_json(&path, &request).await?;
        Ok(())
    }

    /// Creates a CardKit entity and returns its `card_id`.
    ///
    /// A card entity is valid for 14 days and can be referenced by one sent
    /// message. It is the required starting point for later element-level or
    /// streaming CardKit updates.
    pub async fn create_card_entity(&self, card: &Card) -> Result<CardId> {
        card.validate()?;
        let request = CardEntityPayload::from_card(card)?;
        let response: CreateCardEntityResponse =
            self.post_tenant_json(CARD_ENTITY_PATH, &request).await?;
        CardId::new(response.data.card_id)
    }

    /// Replaces all content of a CardKit entity by `card_id`.
    ///
    /// `options.sequence` must be strictly greater than the sequence used by
    /// the previous CardKit operation on the same entity.
    pub async fn update_card_entity(
        &self,
        card_id: &CardId,
        card: &Card,
        options: CardUpdateOptions,
    ) -> Result<()> {
        card_id.validate()?;
        card.validate()?;
        options.validate()?;
        let path = format!("{CARD_ENTITY_PATH}/{}", card_id.as_str());
        let request = UpdateCardEntityRequest {
            card: CardEntityPayload::from_card(card)?,
            sequence: options.sequence,
            uuid: options.uuid,
        };
        let _: Value = self.put_tenant_json(&path, &request).await?;
        Ok(())
    }
}

/// Required sequencing and optional idempotency values for CardKit updates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardUpdateOptions {
    /// Strictly increasing operation sequence for one CardKit entity.
    pub sequence: u32,
    /// Optional request de-duplication key accepted by CardKit update APIs.
    pub uuid: Option<String>,
}

impl CardUpdateOptions {
    /// Creates update options with the required CardKit operation sequence.
    pub fn new(sequence: u32) -> Self {
        Self {
            sequence,
            uuid: None,
        }
    }

    /// Sets the optional CardKit update idempotency key.
    pub fn uuid(mut self, uuid: impl Into<String>) -> Self {
        self.uuid = Some(uuid.into());
        self
    }

    fn validate(&self) -> Result<()> {
        if !(1..=MAX_CARD_UPDATE_SEQUENCE).contains(&self.sequence) {
            return Err(Error::Validation(format!(
                "card update sequence must be between 1 and {MAX_CARD_UPDATE_SEQUENCE}"
            )));
        }
        if let Some(uuid) = &self.uuid {
            if uuid.is_empty() {
                return Err(Error::Validation(
                    "card update uuid must not be empty".to_owned(),
                ));
            }
            if uuid.chars().count() > MAX_CARD_UPDATE_UUID_CHARS {
                return Err(Error::Validation(format!(
                    "card update uuid must be at most {MAX_CARD_UPDATE_UUID_CHARS} characters"
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
struct UpdateMessageCardRequest {
    content: String,
}

#[derive(Debug, Serialize)]
struct CardEntityPayload {
    r#type: &'static str,
    data: String,
}

impl CardEntityPayload {
    fn from_card(card: &Card) -> Result<Self> {
        Ok(Self {
            r#type: "card_json",
            data: serde_json::to_string(card)?,
        })
    }
}

#[derive(Debug, Serialize)]
struct UpdateCardEntityRequest {
    card: CardEntityPayload,
    sequence: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    uuid: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CreateCardEntityResponse {
    data: CreateCardEntityData,
}

#[derive(Debug, Deserialize)]
struct CreateCardEntityData {
    card_id: String,
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::lark_openapi::test_support::{FakeTransport, block_on};
    use crate::lark_openapi::{HttpMethod, HttpResponse};
    use crate::message::{MessageContent, MessageId, Recipient};
    use crate::{Card, CardId, ChannelConfig};

    #[test]
    fn update_message_card_patches_serialized_shared_card() {
        let transport = FakeTransport::new(vec![
            HttpResponse::json(
                200,
                json!({
                    "code": 0,
                    "msg": "ok",
                    "tenant_access_token": "tenant-token-1",
                    "expire": 7200
                }),
            ),
            HttpResponse::json(200, json!({ "code": 0, "msg": "ok", "data": {} })),
        ]);
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());
        let card = Card::builder().markdown("updated").build().expect("card");

        block_on(client.update_message_card(&MessageId("om_123".to_owned()), &card))
            .expect("updated message card");

        let calls = transport.calls();
        assert_eq!(calls[1].method, HttpMethod::Patch);
        assert_eq!(
            calls[1].url.as_str(),
            "https://open.feishu.cn/open-apis/im/v1/messages/om_123"
        );
        let content: Value =
            serde_json::from_str(calls[1].body["content"].as_str().expect("content string"))
                .expect("card json");
        assert_eq!(content["schema"], "2.0");
        assert_eq!(content["config"]["update_multi"], true);
        assert_eq!(content["body"]["elements"][0]["content"], "updated");
    }

    #[test]
    fn update_message_card_rejects_url_path_delimiters_before_authentication() {
        let transport = FakeTransport::new(vec![]);
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());
        let card = Card::builder().text("hello").build().expect("card");

        for message_id in ["om_1/extra", "om_1?x=1", "om_1#fragment", "om_1%2Fextra"] {
            let error =
                block_on(client.update_message_card(&MessageId(message_id.to_owned()), &card))
                    .expect_err("unsafe message_id must fail");
            assert!(matches!(error, Error::Validation(_)));
        }
        assert!(transport.calls().is_empty());
    }

    #[test]
    fn create_card_entity_posts_card_json_and_validates_response_id() {
        let transport = FakeTransport::new(vec![
            HttpResponse::json(
                200,
                json!({
                    "code": 0,
                    "msg": "ok",
                    "tenant_access_token": "tenant-token-1",
                    "expire": 7200
                }),
            ),
            HttpResponse::json(
                200,
                json!({
                    "code": 0,
                    "msg": "ok",
                    "data": { "card_id": "7355372766134157313" }
                }),
            ),
        ]);
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());
        let card = Card::builder().text("hello").build().expect("card");

        let card_id = block_on(client.create_card_entity(&card)).expect("card entity");

        assert_eq!(card_id, CardId("7355372766134157313".to_owned()));
        let calls = transport.calls();
        assert_eq!(calls[1].method, HttpMethod::Post);
        assert_eq!(
            calls[1].url.as_str(),
            "https://open.feishu.cn/open-apis/cardkit/v1/cards"
        );
        assert_eq!(calls[1].body["type"], "card_json");
        let data: Value =
            serde_json::from_str(calls[1].body["data"].as_str().expect("data string"))
                .expect("card json");
        assert_eq!(data["schema"], "2.0");
        assert_eq!(data["body"]["elements"][0]["text"]["content"], "hello");
    }

    #[test]
    fn update_card_entity_puts_sequence_uuid_and_card_json() {
        let transport = FakeTransport::new(vec![
            HttpResponse::json(
                200,
                json!({
                    "code": 0,
                    "msg": "ok",
                    "tenant_access_token": "tenant-token-1",
                    "expire": 7200
                }),
            ),
            HttpResponse::json(200, json!({ "code": 0, "msg": "ok", "data": {} })),
        ]);
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());
        let card = Card::builder().markdown("done").build().expect("card");
        let card_id = CardId::new("7355372766134157313").expect("card id");

        block_on(client.update_card_entity(
            &card_id,
            &card,
            CardUpdateOptions::new(2).uuid("card-update-2"),
        ))
        .expect("updated card entity");

        let calls = transport.calls();
        assert_eq!(calls[1].method, HttpMethod::Put);
        assert_eq!(
            calls[1].url.as_str(),
            "https://open.feishu.cn/open-apis/cardkit/v1/cards/7355372766134157313"
        );
        assert_eq!(calls[1].body["sequence"], 2);
        assert_eq!(calls[1].body["uuid"], "card-update-2");
        assert_eq!(calls[1].body["card"]["type"], "card_json");
        let data: Value =
            serde_json::from_str(calls[1].body["card"]["data"].as_str().expect("data string"))
                .expect("card json");
        assert_eq!(data["body"]["elements"][0]["content"], "done");
    }

    #[test]
    fn update_card_entity_rejects_invalid_sequence_before_authentication() {
        let transport = FakeTransport::new(vec![]);
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());
        let card = Card::builder().text("hello").build().expect("card");
        let card_id = CardId::new("7355372766134157313").expect("card id");

        let error = block_on(client.update_card_entity(&card_id, &card, CardUpdateOptions::new(0)))
            .expect_err("zero sequence must fail");

        assert!(matches!(
            error,
            Error::Validation(message) if message.contains("card update sequence")
        ));
        assert!(transport.calls().is_empty());
    }

    #[test]
    fn update_card_entity_rejects_invalid_uuid_before_authentication() {
        let transport = FakeTransport::new(vec![]);
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());
        let card = Card::builder().text("hello").build().expect("card");
        let card_id = CardId::new("7355372766134157313").expect("card id");

        for uuid in [String::new(), "x".repeat(65)] {
            let error = block_on(client.update_card_entity(
                &card_id,
                &card,
                CardUpdateOptions::new(1).uuid(uuid),
            ))
            .expect_err("invalid card update uuid must fail");
            assert!(matches!(error, Error::Validation(_)));
        }
        assert!(transport.calls().is_empty());
    }

    #[test]
    fn create_message_serializes_card_entity_reference() {
        let transport = FakeTransport::new(vec![
            HttpResponse::json(
                200,
                json!({
                    "code": 0,
                    "msg": "ok",
                    "tenant_access_token": "tenant-token-1",
                    "expire": 7200
                }),
            ),
            HttpResponse::json(
                200,
                json!({
                    "code": 0,
                    "msg": "ok",
                    "data": { "message_id": "om_card" }
                }),
            ),
        ]);
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());

        let message_id = block_on(client.create_message(
            Recipient::Chat("oc_123".to_owned()),
            MessageContent::CardReference {
                card_id: CardId::new("7355372766134157313").expect("card id"),
            },
        ))
        .expect("card reference message");

        assert_eq!(message_id, MessageId("om_card".to_owned()));
        let body = &transport.calls()[1].body;
        assert_eq!(body["msg_type"], "interactive");
        let content: Value =
            serde_json::from_str(body["content"].as_str().expect("content string"))
                .expect("card reference json");
        assert_eq!(
            content,
            json!({ "type": "card", "data": { "card_id": "7355372766134157313" } })
        );
    }

    #[test]
    fn create_message_rejects_invalid_raw_card_before_authentication() {
        let transport = FakeTransport::new(vec![]);
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());

        let error = block_on(client.create_message(
            Recipient::Chat("oc_123".to_owned()),
            MessageContent::Card {
                card: json!({
                    "schema": "1.0",
                    "body": { "elements": [{ "tag": "markdown", "content": "hello" }] }
                }),
            },
        ))
        .expect_err("non-2.0 card must fail");

        assert!(matches!(
            error,
            Error::Validation(message) if message == "card schema must be \"2.0\""
        ));
        assert!(transport.calls().is_empty());
    }
}
