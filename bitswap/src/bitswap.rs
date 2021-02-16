use crate::block::Block;
use crate::control::Control;
use crate::error::BitswapError;
use crate::ledger::{Ledger, Message, Priority};
use crate::protocol::{Handler, PeerEvent};
use crate::stat::Stats;
use crate::{IBlockStore, BS_PROTO_ID};
use cid::Cid;
use futures::channel::{mpsc, oneshot};
use futures::select;
use futures::stream::FusedStream;
use futures::StreamExt;
use libp2p_rs::core::PeerId;
use libp2p_rs::runtime::task;
use libp2p_rs::swarm::Control as Swarm_Control;
use libp2p_rs::traits::WriteEx;
use std::collections::HashMap;
use std::mem;
use std::sync::Arc;

pub(crate) enum ControlCommand {
    WantBlock(Cid, oneshot::Sender<oneshot::Receiver<Block>>),
    CancelBlock(Cid),
    WantList(oneshot::Sender<Result<Vec<(Cid, Priority)>>>),
}

pub struct Bitswap {
    // Used to open stream.
    swarm: Option<Swarm_Control>,

    /// block store
    blockstore: IBlockStore,

    // New peer is connected or peer is dead.
    peer_tx: mpsc::UnboundedSender<PeerEvent>,
    peer_rx: mpsc::UnboundedReceiver<PeerEvent>,

    // Used to recv incoming rpc message.
    incoming_tx: mpsc::UnboundedSender<(PeerId, Message)>,
    incoming_rx: mpsc::UnboundedReceiver<(PeerId, Message)>,

    // Used to pub/sub/ls/peers.
    control_tx: mpsc::UnboundedSender<ControlCommand>,
    control_rx: mpsc::UnboundedReceiver<ControlCommand>,

    /// Wanted blocks
    wanted_blocks: HashMap<Cid, Vec<oneshot::Sender<Block>>>,

    /// Ledger
    connected_peers: HashMap<PeerId, Ledger>,

    /// Statistics related to peers.
    stats: HashMap<PeerId, Arc<Stats>>,
}

type Result<T> = std::result::Result<T, BitswapError>;

impl Bitswap {
    pub fn new(blockstore: IBlockStore) -> Self {
        let (peer_tx, peer_rx) = mpsc::unbounded();
        let (incoming_tx, incoming_rx) = mpsc::unbounded();
        let (control_tx, control_rx) = mpsc::unbounded();
        Bitswap {
            swarm: None,
            blockstore,
            peer_tx,
            peer_rx,
            incoming_tx,
            incoming_rx,
            control_tx,
            control_rx,
            wanted_blocks: Default::default(),
            connected_peers: Default::default(),
            stats: Default::default(),
        }
    }

    /// Get handler of floodsub, swarm will call "handle" func after muxer negotiate success.
    pub fn handler(&self) -> Handler {
        Handler::new(self.incoming_tx.clone(), self.peer_tx.clone())
    }

    /// Get control of floodsub, which can be used to publish or subscribe.
    pub fn control(&self) -> Control {
        Control::new(self.control_tx.clone())
    }

    /// Start message process loop.
    pub fn start(mut self, control: Swarm_Control) {
        self.swarm = Some(control);

        // well, self 'move' explicitly,
        let mut bitswap = self;
        task::spawn(async move {
            let _ = bitswap.process_loop().await;
        });
    }

    /// Message Process Loop.
    pub async fn process_loop(&mut self) -> Result<()> {
        let result = self.next().await;

        if !self.peer_rx.is_terminated() {
            self.peer_rx.close();
            while self.peer_rx.next().await.is_some() {
                // just drain
            }
        }

        if !self.incoming_rx.is_terminated() {
            self.incoming_rx.close();
            while self.incoming_rx.next().await.is_some() {
                // just drain
            }
        }

        if !self.control_rx.is_terminated() {
            self.control_rx.close();
            while let Some(cmd) = self.control_rx.next().await {
                match cmd {
                    ControlCommand::WantBlock(_, _) => {}
                    ControlCommand::CancelBlock(_) => {}
                    ControlCommand::WantList(_) => {}
                }
            }
        }

        result
    }

    async fn next(&mut self) -> Result<()> {
        loop {
            select! {
                cmd = self.peer_rx.next() => {
                    self.handle_peer_event(cmd).await?;
                }
                msg = self.incoming_rx.next() => {
                    if let Some((source, message)) = msg {
                        self.handle_incoming_message(source, message).await?;
                    }
                }
                cmd = self.control_rx.next() => {
                    self.handle_control_command(cmd).await?;
                }
            }

            self.send_messages().await;
        }
    }

    async fn send_messages(&mut self) {
        for (peer_id, ledger) in &mut self.connected_peers {
            if let Some(message) = ledger.send() {
                if let Some(peer_stats) = self.stats.get_mut(peer_id) {
                    peer_stats.update_outgoing(message.blocks.len() as u64);
                }

                // send meaasge
                send_message(&mut self.swarm, peer_id.clone(), message).await;
            }
        }
    }

    async fn handle_peer_event(&mut self, evt: Option<PeerEvent>) -> Result<()> {
        match evt {
            Some(PeerEvent::NewPeer(p)) => {
                let ledger = Ledger::new();
                self.stats.entry(p.clone()).or_default();
                self.connected_peers.insert(p.clone(), ledger);
                self.send_want_list(p).await;
            }
            Some(PeerEvent::DeadPeer(p)) => {
                self.connected_peers.remove(&p);
            }
            None => {}
        }
        Ok(())
    }

    async fn handle_incoming_message(
        &mut self,
        source: PeerId,
        mut message: Message,
    ) -> Result<()> {
        let current_wantlist = self.local_wantlist();

        let ledger = self
            .connected_peers
            .get_mut(&source)
            .expect("Peer not in ledger?!");

        // Process the incoming cancel list.
        for cid in message.cancel() {
            ledger.received_want_list.remove(cid);
        }

        // Process the incoming wantlist.
        for (cid, priority) in message
            .want()
            .iter()
            .filter(|&(cid, _)| !current_wantlist.iter().map(|c| c).any(|c| c == cid))
        {
            ledger.received_want_list.insert(cid.to_owned(), *priority);
            if let Ok(Some(block)) = self.blockstore.get(cid) {
                ledger.add_block(block);
            }
        }

        // Process the incoming blocks.
        // TODO: send block to any peer who want
        for block in mem::take(&mut message.blocks) {
            self.handle_received_block(block);
        }

        Ok(())
    }

    fn handle_received_block(&mut self, block: Block) {
        // publish block to all subscribers
        self.wanted_blocks.remove(&block.cid).map(|txs| {
            txs.into_iter().for_each(|tx| {
                // some tx may be dropped, regardless
                let _ = tx.send(block.clone());
            })
        });

        // cancel want
        for (_peer_id, ledger) in self.connected_peers.iter_mut() {
            ledger.cancel_block(&block.cid);
        }
    }

    async fn handle_control_command(&mut self, cmd: Option<ControlCommand>) -> Result<()> {
        match cmd {
            Some(ControlCommand::WantBlock(cid, reply)) => {
                let (tx, rx) = oneshot::channel();
                self.want_block(cid, 1, tx);
                let _ = reply.send(rx);
            }
            Some(ControlCommand::CancelBlock(cid)) => self.cancel_block(&cid),
            Some(ControlCommand::WantList(reply)) => {
                let list = self.local_wantlist().into_iter().map(|cid| (cid, 1)).collect();
                let _ = reply.send(Ok(list));
            },
            None => {}
        }
        Ok(())
    }
}

impl Bitswap {
    /// Queues the wanted block for all peers.
    ///
    /// A user request
    pub fn want_block(&mut self, cid: Cid, priority: Priority, tx: oneshot::Sender<Block>) {
        for (_peer_id, ledger) in self.connected_peers.iter_mut() {
            ledger.want_block(&cid, priority);
        }
        self.wanted_blocks.entry(cid).or_insert(vec![]).push(tx);
    }

    /// Removes the block from our want list and updates all peers.
    ///
    /// Can be either a user request or be called when the block
    /// was received.
    pub fn cancel_block(&mut self, cid: &Cid) {
        for (_peer_id, ledger) in self.connected_peers.iter_mut() {
            ledger.cancel_block(cid);
        }
        self.wanted_blocks.remove(cid);
    }

    /// Return the wantlist of the local node
    pub fn local_wantlist(&self) -> Vec<Cid> {
        self.wanted_blocks
            .iter()
            .map(|(cid, _)| cid.clone())
            .collect()
    }

    /// Sends the wantlist to the peer.
    async fn send_want_list(&mut self, peer_id: PeerId) {
        if !self.wanted_blocks.is_empty() {
            // FIXME: this can produce too long a message
            // FIXME: we should shard these across all of our peers by some logic; also, peers may
            // have been discovered to provide some specific wantlist item
            let mut message = Message::default();
            for (cid, _) in &self.wanted_blocks {
                // TODO: set priority
                message.want_block(cid, 1);
            }

            send_message(&mut self.swarm, peer_id, message).await;
        }
    }
}

async fn send_message(swarm: &mut Option<Swarm_Control>, peer_id: PeerId, message: Message) {
    // send meaasge
    let stream = swarm
        .as_mut()
        .unwrap()
        .new_stream(peer_id.clone(), vec![BS_PROTO_ID.into()])
        .await;

    if let Ok(mut s) = stream {
        s.write_one(message.to_bytes().as_ref()).await;
    }
}
