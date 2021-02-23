use std::sync::Arc;
use std::collections::HashMap;
use std::time::Duration;
use cid::Cid;
use futures::channel::{mpsc, oneshot};
use futures::{select, SinkExt};
use futures::StreamExt;

use libp2p_rs::core::PeerId;
use libp2p_rs::runtime::task;
use libp2p_rs::swarm::Control as SwarmControl;

use crate::block::Block;
use crate::control::Control;
use crate::error::BitswapError;
use crate::ledger::{Ledger, Message, Priority};
use crate::protocol::{Handler, ProtocolEvent, send_message};
use crate::stat::Stats;
use crate::BsBlockStore;
use libp2p_rs::swarm::protocol_handler::{ProtocolImpl, IProtocolHandler};
use libp2p_rs::core::routing::Routing;

const WANT_DEADLINE: Duration = Duration::from_secs(30);

pub(crate) enum ControlCommand {
    WantBlock(Cid, oneshot::Sender<Result<Block>>),
    HasBlock(Cid, oneshot::Sender<Result<()>>),
    CancelBlock(Cid, oneshot::Sender<Result<()>>),
    WantList(Option<PeerId>, oneshot::Sender<Result<Vec<(Cid, Priority)>>>),
    Peers(oneshot::Sender<Result<Vec<PeerId>>>),
    Stats(oneshot::Sender<Result<Stats>>),
}

pub struct Bitswap<TBlockStore, TRouting> {
    // Swarm controller.
    swarm: Option<SwarmControl>,

    // routing, Kad-DHT
    routing: TRouting,

    // blockstore
    blockstore: TBlockStore,

    // New peer is connected or peer is dead.
    peer_tx: mpsc::UnboundedSender<ProtocolEvent>,
    peer_rx: mpsc::UnboundedReceiver<ProtocolEvent>,

    // Used to recv incoming rpc message.
    incoming_tx: mpsc::UnboundedSender<(PeerId, Message)>,
    incoming_rx: mpsc::UnboundedReceiver<(PeerId, Message)>,

    // Used to pub/sub/ls/peers.
    control_tx: mpsc::UnboundedSender<ControlCommand>,
    control_rx: mpsc::UnboundedReceiver<ControlCommand>,

    want_deadline: Duration,

    /// Wanted blocks
    ///
    /// The oneshot::Sender is used to send the block back to the API users.
    wanted_blocks: HashMap<Cid, Vec<oneshot::Sender<Block>>>,

    /// Ledger
    connected_peers: HashMap<PeerId, Ledger>,

    /// Statistics related to peers.
    stats: HashMap<PeerId, Arc<Stats>>,
}

type Result<T> = std::result::Result<T, BitswapError>;

impl<TBlockStore, TRouting> Bitswap<TBlockStore, TRouting>
    where
        TBlockStore: BsBlockStore,
        TRouting: Routing + Clone + 'static
{
    pub fn new(blockstore: TBlockStore, routing: TRouting) -> Self {
        let (peer_tx, peer_rx) = mpsc::unbounded();
        let (incoming_tx, incoming_rx) = mpsc::unbounded();
        let (control_tx, control_rx) = mpsc::unbounded();
        Bitswap {
            swarm: None,
            routing,
            blockstore,
            peer_tx,
            peer_rx,
            incoming_tx,
            incoming_rx,
            control_tx,
            control_rx,
            want_deadline: WANT_DEADLINE,
            wanted_blocks: Default::default(),
            connected_peers: Default::default(),
            stats: Default::default(),
        }
    }

    /// Get control of floodsub, which can be used to publish or subscribe.
    pub fn control(&self) -> Control {
        Control::new(self.control_tx.clone())
    }

    /// Message Process Loop.
    pub async fn process_loop(&mut self) -> Result<()> {
        loop {
            select! {
                cmd = self.peer_rx.next() => {
                    self.handle_event(cmd);
                }
                msg = self.incoming_rx.next() => {
                    if let Some((source, message)) = msg {
                        self.handle_incoming_message(source, message).await;
                    }
                }
                cmd = self.control_rx.next() => {
                    self.handle_control_command(cmd)?;
                }
            }
        }
    }

    fn send_message_to(&mut self, peer_id: PeerId, message: Message) {
        if let Some(peer_stats) = self.stats.get_mut(&peer_id) {
            peer_stats.update_outgoing(message.num_of_blocks() as u64, message.bytes_of_blocks() as u64);
        }

        // spwan a task to send the message
        let swarm = self.swarm.clone().expect("swarm??");
        task::spawn(async move {
            let _ = send_message(swarm, peer_id, message).await;
        });
    }

    fn broadcast_messages(&mut self) {
        for (peer_id, ledger) in &mut self.connected_peers {
            if let Some(message) = ledger.send() {
                if let Some(peer_stats) = self.stats.get_mut(peer_id) {
                    peer_stats.update_outgoing(message.num_of_blocks() as u64, message.bytes_of_blocks() as u64);
                }

                // spwan a task to send the message
                let swarm = self.swarm.clone().expect("swarm??");
                let peer_id = *peer_id;
                task::spawn(async move {
                    let _ = send_message(swarm, peer_id, message).await;
                });
            }
        }
    }

    fn handle_event(&mut self, evt: Option<ProtocolEvent>) {
        match evt {
            Some(ProtocolEvent::Blocks(peer, blocks)) => {
                log::debug!("blockstore reports {} block(s) for {:?}", blocks.len(), peer);
                let ledger = self
                    .connected_peers
                    .get_mut(&peer)
                    .expect("Peer without ledger?!");
                //self.s
                blocks.into_iter().for_each(|block| ledger.add_block(block));

                if let Some(message) = ledger.send() {
                    self.send_message_to(peer, message);
                }
            }
            Some(ProtocolEvent::NewPeer(p)) => {
                log::debug!("{:?} connected", p);
                // make a ledge for the peer and send wantlist to it
                let ledger = Ledger::new();
                self.connected_peers.insert(p.clone(), ledger);
                self.stats.entry(p.clone()).or_default();
                self.send_want_list(p);
            }
            Some(ProtocolEvent::DeadPeer(p)) => {
                log::debug!("{:?} disconnected", p);
                self.connected_peers.remove(&p);
            }
            None => {}
        }
    }

    async fn handle_incoming_message(
        &mut self,
        source: PeerId,
        mut message: Message,
    ) {
        log::debug!("incoming message: from {:?}, w={} c={} b={}", source,
                    message.want().len(), message.cancel().len(), message.blocks().len());

        let current_wantlist = self.local_wantlist();

        let ledger = self
            .connected_peers
            .get_mut(&source)
            .expect("Peer without ledger?!");

        // Process the incoming cancel list.
        for cid in message.cancel() {
            ledger.received_want_list.remove(cid);
        }

        // Process the incoming wantlist.
        let mut to_check = vec![];
        for (cid, priority) in message
            .want()
            .iter()
            .filter(|&(cid, _)| !current_wantlist.contains(&cid))
        {
            ledger.received_want_list.insert(cid.to_owned(), *priority);
            to_check.push(cid.to_owned());
        }

        if !to_check.is_empty() {
            // ask blockstore for the wanted blocks
            log::debug!("{:?} asking for {} block(s), checking blockstore", source, to_check.len());
            let blockstore = self.blockstore.clone();
            let mut poster = self.peer_tx.clone();
            task::spawn(async move {
                let mut blocks = vec![];
                for cid in to_check {
                    if let Ok(Some(block)) = blockstore.get(&cid).await {
                        log::debug!("block {} found in blockstore", cid);
                        blocks.push(block);
                    }
                }
                if !blocks.is_empty() {
                    let _ = poster.send(ProtocolEvent::Blocks(source, blocks)).await;
                }
            });
        }

        // Process the incoming blocks.
        // TODO: send block to any peer who want
        let blocks = message.take_blocks();
        if !blocks.is_empty() {
            self.handle_received_blocks(source, blocks);
        }
    }

    fn handle_received_blocks(&mut self, source: PeerId, blocks: Vec<Block>) {
        log::debug!("received {} block(s) from {:?}", blocks.len(), source);

        for block in &blocks {
            // publish block to all pending API users
            self.wanted_blocks.remove(&block.cid).map(|txs| {
                txs.into_iter().for_each(|tx| {
                    // some tx may be dropped, regardless
                    log::debug!("wake up API client with {:?} from {:?}", block.cid, source);
                    let _ = tx.send(block.clone());
                })
            });

            // cancel want
            for (_peer_id, ledger) in self.connected_peers.iter_mut() {
                ledger.cancel_block(&block.cid);
            }
        }

        // put all blocks onto blockstore
        // note that 'blocks' are moved into the task
        let blockstore = self.blockstore.clone();
        let peer_stats = Arc::clone(&self.stats.get(&source).unwrap());
        task::spawn(async move {
            for block in blocks {
                let bytes = block.data().len() as u64;
                let res = blockstore.put(block).await;
                match res {
                    Ok((_, true)) => {
                        peer_stats.update_incoming_unique(bytes);
                    },
                    Ok((_, false)) => {
                        peer_stats.update_incoming_duplicate(bytes);
                    },
                    Err(e) => {
                        log::info!("Got block from {:?} but failed to store it: {}", source, e);
                    }
                }
            }
        });
    }

    fn handle_control_command(&mut self, cmd: Option<ControlCommand>) -> Result<()> {
        match cmd {
            Some(ControlCommand::WantBlock(cid, reply)) => {
                self.want_block(cid, 1, reply);
            }
            Some(ControlCommand::HasBlock(cid, reply)) => {
                self.has_block(cid, reply);
            }
            Some(ControlCommand::CancelBlock(cid, reply)) => {
                self.cancel_block(&cid, reply)
            },
            Some(ControlCommand::WantList(peer, reply)) => {
                if let Some(peer_id) = peer {
                    let list = self.peer_wantlist(&peer_id)
                        .unwrap_or_default();
                    let _ = reply.send(Ok(list));
                } else {
                    let list = self.local_wantlist()
                        .into_iter()
                        .map(|cid| (cid, 1))
                        .collect();
                    let _ = reply.send(Ok(list));
                }
            },
            Some(ControlCommand::Peers(reply)) => {
                let _ = reply.send(Ok(self.peers()));
            },
            Some(ControlCommand::Stats(reply)) => {
                let _ = reply.send(Ok(self.stats()));
            },
            None => {
                // control channel closed, exit the main loop
                return Err(BitswapError::Closing);
            }
        }
        Ok(())
    }

    /// Retrieves the wanted block.
    ///
    /// A user request
    pub fn want_block(&mut self, cid: Cid, priority: Priority, reply: oneshot::Sender<Result<Block>>) {
        log::debug!("bitswap want block {} ", cid);

        // TODO: should run a dedicated peer manager for find_providers...
        let mut routing = self.routing.clone();
        let mut swarm = self.swarm.clone().expect("Swarm??");
        let key = cid.to_bytes();
        task::spawn(async move {
            let r = routing.find_providers(key, 1).await;
            if let Ok(peers) = r {
                // open a connection toward the providers, so that bitswap could be happy
                // to fetch the wanted blocks
                for peer in peers {
                    let _ = swarm.new_connection_no_routing(peer).await;
                }
            }
        });

        for (_peer_id, ledger) in self.connected_peers.iter_mut() {
            ledger.want_block(&cid, priority);
        }
        let (tx, rx) = oneshot::channel();
        self.wanted_blocks.entry(cid).or_insert(vec![]).push(tx);

        // ask all known peers for the wanted block
        self.broadcast_messages();

        let deadline = self.want_deadline;
        task::spawn(async move {
            let r = task::timeout(deadline, rx).await;
            if let Ok(block) = r {
                let _ = reply.send(block.map_err(|e| BitswapError::Cancel(e)));
            } else {
                let _ = reply.send(Err(BitswapError::Timeout));
            }
        });
    }

    /// Announces a new block.
    ///
    /// A user request
    pub fn has_block(&mut self, cid: Cid, reply: oneshot::Sender<Result<()>>) {
        log::debug!("bitswap has block {} ", cid);

        // firstly, cancel this new block and remove it from our wantlist
        for (_peer_id, ledger) in self.connected_peers.iter_mut() {
            ledger.cancel_block(&cid);
        }
        self.wanted_blocks.remove(&cid);

        // announce via routing
        let mut routing = self.routing.clone();
        task::spawn(async move {
            let _ = routing.provide(cid.to_bytes()).await;
        });

        let _ = reply.send(Ok(()));
    }

    /// Removes the block from our want list and updates all peers.
    ///
    /// Can be either a user request or be called when the block
    /// was received.
    pub fn cancel_block(&mut self, cid: &Cid, reply: oneshot::Sender<Result<()>>) {
        log::debug!("bitswap cancel block {} ", cid);
        for (_peer_id, ledger) in self.connected_peers.iter_mut() {
            ledger.cancel_block(cid);
        }
        self.wanted_blocks.remove(cid);
        let _ = reply.send(Ok(()));
    }

    /// Returns the wantlist of a peer, if known
    pub fn peer_wantlist(&self, peer: &PeerId) -> Option<Vec<(Cid, Priority)>> {
        self.connected_peers.get(peer).map(Ledger::wantlist)
    }

    /// Returns the wantlist of the local node
    pub fn local_wantlist(&self) -> Vec<Cid> {
        self.wanted_blocks
            .iter()
            .map(|(cid, _)| cid.clone())
            .collect()
    }

    /// Returns the connected peers.
    pub fn peers(&self) -> Vec<PeerId> {
        self.connected_peers.keys().cloned().collect()
    }

    /// Returns the statistics of bitswap.
    pub fn stats(&self) -> Stats {
        self.stats
            .values()
            .fold(Stats::default(), |acc, peer_stats| {
                acc.add_assign(&peer_stats);
                acc
            })
    }

    /// Sends the wantlist to the peer.
    fn send_want_list(&mut self, peer_id: PeerId) {
        if !self.wanted_blocks.is_empty() {
            // FIXME: this can produce too long a message
            // FIXME: we should shard these across all of our peers by some logic; also, peers may
            // have been discovered to provide some specific wantlist item
            let mut message = Message::default();
            for (cid, _) in &self.wanted_blocks {
                // TODO: set priority
                message.want_block(cid, 1);
            }

            // spwan a task to send the message
            let swarm = self.swarm.clone().expect("swarm??");
            task::spawn(async move {
                let _ = send_message(swarm, peer_id, message).await;
            });
        }
    }
}

impl<TBlockStore, TRouting> ProtocolImpl for Bitswap<TBlockStore, TRouting>
    where
        TBlockStore: BsBlockStore,
        TRouting: Routing + Clone + 'static
{
    /// Get handler of floodsub, swarm will call "handle" func after muxer negotiate success.
    fn handler(&self) -> IProtocolHandler {
        Box::new(Handler::new(self.incoming_tx.clone(), self.peer_tx.clone()))
    }

    /// Start message process loop.
    fn start(mut self, swarm: SwarmControl) -> Option<task::TaskHandle<()>> where
        Self: Sized, {
        self.swarm = Some(swarm);

        // well, self 'move' explicitly,
        let mut bitswap = self;

        Some(task::spawn(async move {
            log::info!("starting bitswap main loop...");
            let _ = bitswap.process_loop().await;
            log::info!("exiting bitswap main loop...");
        }))
    }
}