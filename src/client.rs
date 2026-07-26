use crate::Result;
use crate::card::Card;
use crate::event::ChannelEvent;
use crate::media::{DownloadedResource, ResourceDescriptor};
use crate::message::{MessageContent, MessageId, Recipient};

pub trait ChannelClient {
    fn send_message(
        &self,
        recipient: Recipient,
        content: MessageContent,
    ) -> impl std::future::Future<Output = Result<MessageId>> + Send;

    fn create_card(&self, card: Card) -> impl std::future::Future<Output = Result<String>> + Send;

    fn update_card(
        &self,
        card_id: String,
        card: Card,
    ) -> impl std::future::Future<Output = Result<()>> + Send;

    fn download_resource(
        &self,
        resource: ResourceDescriptor,
    ) -> impl std::future::Future<Output = Result<DownloadedResource>> + Send;

    fn next_event(&self) -> impl std::future::Future<Output = Result<Option<ChannelEvent>>> + Send;
}

pub trait ChannelClientExt: ChannelClient {
    fn send_text(
        &self,
        recipient: Recipient,
        text: impl Into<String>,
    ) -> impl std::future::Future<Output = Result<MessageId>> + Send {
        self.send_message(recipient, MessageContent::Text { text: text.into() })
    }
}

impl<T: ChannelClient + ?Sized> ChannelClientExt for T {}

#[cfg(test)]
mod tests {
    use std::future::Future;

    use super::*;
    use crate::media::ResourceType;

    struct MemoryOnlyClient;

    impl ChannelClient for MemoryOnlyClient {
        async fn send_message(
            &self,
            _recipient: Recipient,
            _content: MessageContent,
        ) -> Result<MessageId> {
            unreachable!()
        }

        async fn create_card(&self, _card: Card) -> Result<String> {
            unreachable!()
        }

        async fn update_card(&self, _card_id: String, _card: Card) -> Result<()> {
            unreachable!()
        }

        async fn download_resource(
            &self,
            _resource: ResourceDescriptor,
        ) -> Result<DownloadedResource> {
            Ok(DownloadedResource {
                bytes: Vec::new(),
                content_type: None,
                content_disposition: None,
            })
        }

        async fn next_event(&self) -> Result<Option<ChannelEvent>> {
            unreachable!()
        }
    }

    #[test]
    fn channel_client_download_contract_is_memory_only() {
        fn assert_download_future(
            _future: impl Future<Output = Result<DownloadedResource>> + Send,
        ) {
        }

        let descriptor = ResourceDescriptor {
            message_id: "om_test".to_owned(),
            resource_type: ResourceType::Image,
            file_key: None,
            image_key: Some("img_test".to_owned()),
            file_name: None,
            duration_ms: None,
        };
        assert_download_future(MemoryOnlyClient.download_resource(descriptor));
    }
}
