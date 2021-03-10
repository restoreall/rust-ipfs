use libp2p_rs::core::PeerId;
use libp2p_rs::floodsub::protocol::FloodsubMessage;
use libp2p_rs::floodsub::subscription::Subscription;
use std::sync::Arc;

/// Adaptation hopefully supporting both Floodsub and Gossipsub Messages in the future
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct PubsubMessage {
    /// Peer address of the message sender.
    pub source: PeerId,
    /// The message data.
    pub data: Vec<u8>,
    /// The sequence number of the message.
    // this could be an enum for gossipsub message compat, it uses u64, though the floodsub
    // sequence numbers looked like 8 bytes in testing..
    pub sequence_number: Vec<u8>,
    /// The recepients of the message (topic IDs).
    // TODO: gossipsub uses topichashes, haven't checked if we could have some unifying abstraction
    // or if we should have a hash to name mapping internally?
    pub topics: Vec<String>,
}

impl From<FloodsubMessage> for PubsubMessage {
    fn from(
        FloodsubMessage {
            source,
            data,
            sequence_number,
            topics,
        }: FloodsubMessage,
    ) -> Self {
        PubsubMessage {
            source,
            data,
            sequence_number,
            topics: topics.into_iter().map(String::from).collect(),
        }
    }
}

/// Stream of a pubsub messages.
pub struct SubscriptionStream(Subscription);

impl SubscriptionStream {
    pub async fn next(&mut self) -> Option<Arc<PubsubMessage>> {
        self.0
            .next()
            .await
            .map(|m| Arc::new(m.as_ref().clone().into()))
    }
}

impl From<Subscription> for SubscriptionStream {
    fn from(s: Subscription) -> Self {
        Self(s)
    }
}
