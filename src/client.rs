use crate::Result;
use crate::card::Card;
use crate::event::ChannelEvent;
use crate::media::ResourceDescriptor;
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
        path: String,
    ) -> impl std::future::Future<Output = Result<()>> + Send;

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
