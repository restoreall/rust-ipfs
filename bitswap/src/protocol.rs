use crate::ledger::Message;
use crate::BS_PROTO_ID;
use async_trait::async_trait;
use futures::channel::mpsc;
use futures::SinkExt;
use libp2p_rs::core::upgrade::UpgradeInfo;
use libp2p_rs::core::{PeerId, ProtocolId};
use libp2p_rs::runtime::task;
use libp2p_rs::swarm::connection::Connection;
use libp2p_rs::swarm::protocol_handler::{IProtocolHandler, Notifiee, ProtocolHandler};
use libp2p_rs::swarm::substream::Substream;
use libp2p_rs::traits::ReadEx;
use std::error::Error;

const MAX_BUF_SIZE: usize = 524_288;

pub(crate) enum PeerEvent {
    NewPeer(PeerId),
    DeadPeer(PeerId),
}

#[derive(Clone)]
pub struct Handler {
    incoming_tx: mpsc::UnboundedSender<(PeerId, Message)>,
    new_peer: mpsc::UnboundedSender<PeerEvent>,
}

impl Handler {
    pub(crate) fn new(
        incoming_tx: mpsc::UnboundedSender<(PeerId, Message)>,
        new_peer: mpsc::UnboundedSender<PeerEvent>,
    ) -> Self {
        Handler {
            incoming_tx,
            new_peer,
        }
    }
}

impl UpgradeInfo for Handler {
    type Info = ProtocolId;

    fn protocol_info(&self) -> Vec<Self::Info> {
        vec![BS_PROTO_ID.into()]
    }
}

impl Notifiee for Handler {
    fn connected(&mut self, conn: &mut Connection) {
        let peer_id = conn.remote_peer();
        let mut new_peers = self.new_peer.clone();
        task::spawn(async move {
            let _ = new_peers.send(PeerEvent::NewPeer(peer_id)).await;
        });
    }
}

#[async_trait]
impl ProtocolHandler for Handler {
    async fn handle(
        &mut self,
        mut stream: Substream,
        _info: <Self as UpgradeInfo>::Info,
    ) -> Result<(), Box<dyn Error>> {
        log::trace!("Handle stream from {}", stream.remote_peer());
        loop {
            let packet = stream.read_one(MAX_BUF_SIZE).await?;
            let message = Message::from_bytes(&packet)?;
            let peer = stream.remote_peer();
            self.incoming_tx.send((peer, message)).await;
        }
    }

    fn box_clone(&self) -> IProtocolHandler {
        Box::new(self.clone())
    }
}
