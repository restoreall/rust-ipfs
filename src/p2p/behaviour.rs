use super::pubsub::Pubsub;
use super::swarm::{Connection, Disconnector, SwarmApi};
use crate::config::BOOTSTRAP_NODES;
use crate::p2p::{MultiaddrWithPeerId, SwarmOptions};
use crate::repo::{BlockPut, Repo};
use crate::subscription::{SubscriptionFuture, SubscriptionRegistry};
use crate::IpfsTypes;
use anyhow::anyhow;
use cid::Cid;
use ipfs_bitswap::{Bitswap, BitswapEvent};
use libp2p::core::{Multiaddr, PeerId};
use libp2p::identify::{Identify, IdentifyEvent};
use libp2p::kad::record::{store::MemoryStore, Key, Record};
use libp2p::kad::{Kademlia, KademliaConfig, KademliaEvent, Quorum};
// use libp2p::mdns::{MdnsEvent, TokioMdns};
use libp2p::ping::{Ping, PingEvent};
// use libp2p::swarm::toggle::Toggle;
use libp2p::swarm::{NetworkBehaviour, NetworkBehaviourEventProcess};
use multibase::Base;
use std::{convert::TryInto, sync::Arc};
use tokio::task;
use libp2p_rs::core::PeerId;
use libp2p_rs::kad::Record;


/// Represents the result of a Kademlia query.
#[derive(Debug, Clone, PartialEq)]
pub enum KadResult {
    /// The query has been exhausted.
    Complete,
    /// The query successfully returns `GetClosestPeers` or `GetProviders` results.
    Peers(Vec<PeerId>),
    /// The query successfully returns a `GetRecord` result.
    Records(Vec<Record>),
}

/*
impl<Types: IpfsTypes> NetworkBehaviourEventProcess<MdnsEvent> for Behaviour<Types> {
    fn inject_event(&mut self, event: MdnsEvent) {
        match event {
            MdnsEvent::Discovered(list) => {
                for (peer, addr) in list {
                    trace!("mdns: Discovered peer {}", peer.to_base58());
                    self.add_peer(peer, addr);
                }
            }
            MdnsEvent::Expired(list) => {
                for (peer, _) in list {
                    trace!("mdns: Expired peer {}", peer.to_base58());
                    self.remove_peer(&peer);
                }
            }
        }
    }
}
*/
/*
impl<Types: IpfsTypes> NetworkBehaviourEventProcess<BitswapEvent> for Behaviour<Types> {
    fn inject_event(&mut self, event: BitswapEvent) {
        match event {
            BitswapEvent::ReceivedBlock(peer_id, block) => {
                let repo = self.repo.clone();
                let peer_stats = Arc::clone(&self.bitswap.stats.get(&peer_id).unwrap());
                task::spawn(async move {
                    let bytes = block.data().len() as u64;
                    let res = repo.put_block(block.clone()).await;
                    match res {
                        Ok((_, uniqueness)) => match uniqueness {
                            BlockPut::NewBlock => peer_stats.update_incoming_unique(bytes),
                            BlockPut::Existed => peer_stats.update_incoming_duplicate(bytes),
                        },
                        Err(e) => {
                            debug!(
                                "Got block {} from peer {} but failed to store it: {}",
                                block.cid,
                                peer_id.to_base58(),
                                e
                            );
                        }
                    };
                });
            }
            BitswapEvent::ReceivedWant(peer_id, cid, priority) => {
                info!(
                    "Peer {} wants block {} with priority {}",
                    peer_id, cid, priority
                );

                let queued_blocks = self.bitswap().queued_blocks.clone();
                let repo = self.repo.clone();

                task::spawn(async move {
                    match repo.get_block_now(&cid).await {
                        Ok(Some(block)) => {
                            let _ = queued_blocks.unbounded_send((peer_id, block));
                        }
                        Ok(None) => {}
                        Err(err) => {
                            warn!(
                                "Peer {} wanted block {} but we failed: {}",
                                peer_id.to_base58(),
                                cid,
                                err,
                            );
                        }
                    }
                });
            }
            BitswapEvent::ReceivedCancel(..) => {}
        }
    }
}*/
