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
