use cid::Cid;
use futures::channel::{mpsc, oneshot};
use futures::SinkExt;

use libp2p_rs::core::PeerId;

use crate::bitswap::ControlCommand;
use crate::block::Block;
use crate::{Priority, Stats};
use crate::error::BitswapError;

#[derive(Clone)]
pub struct Control(mpsc::UnboundedSender<ControlCommand>);

impl Control {
    pub(crate) fn new(tx: mpsc::UnboundedSender<ControlCommand>) -> Self {
        Control(tx)
    }

    /// Queues the wanted block for all peers.
    ///
    /// A user request
    pub async fn want_block(&mut self, cid: Cid, _priority: Priority) -> oneshot::Receiver<Block> {
        let (tx, rx) = oneshot::channel();
        self.0.send(ControlCommand::WantBlock(cid, tx)).await;
        rx.await.unwrap()
    }

    /// Returns the wantlist of local if peer is `None`, or the wantlst of the peer specified.
    ///
    /// A user request
    pub async fn wantlist(&mut self, peer: Option<PeerId>) -> Result<Vec<(Cid, Priority)>, BitswapError> {
        let (tx, rx) = oneshot::channel();
        self.0.send(ControlCommand::WantList(peer, tx)).await?;
        rx.await?
    }

    /// Returns the connected peers.
    ///
    /// A user request
    pub async fn peers(&mut self) -> Result<Vec<PeerId>, BitswapError> {
        let (tx, rx) = oneshot::channel();
        self.0.send(ControlCommand::Peers(tx)).await?;
        rx.await?
    }

    /// Returns the bitswap statistics per peer basis.
    ///
    /// A user request
    pub async fn stats(&mut self) -> Result<Stats, BitswapError> {
        let (tx, rx) = oneshot::channel();
        self.0.send(ControlCommand::Stats(tx)).await?;
        rx.await?
    }
}
