use crate::bitswap::ControlCommand;
use crate::block::Block;
use cid::Cid;
use futures::channel::mpsc;
use futures::channel::oneshot;
use futures::SinkExt;
use crate::Priority;
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

    /// Queues the wanted block for all peers.
    ///
    /// A user request
    pub async fn wantlist(&mut self) -> Result<Vec<(Cid, Priority)>, BitswapError> {
        let (tx, rx) = oneshot::channel();
        self.0.send(ControlCommand::WantList(tx)).await;
        rx.await.unwrap()
    }
}
