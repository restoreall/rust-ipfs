//! P2P handling for IPFS nodes.
use crate::repo::Repo;
use crate::{IpfsOptions, IpfsTypes, Cid};
use std::io;
use std::time::Duration;
use std::sync::Arc;
use tracing::Span;

pub(crate) mod addr;
pub(crate) mod pubsub;
mod swarm;

use std::error::Error;


pub use addr::{MultiaddrWithPeerId, MultiaddrWithoutPeerId};
pub use {swarm::Connection};
use libp2p_rs::core::identity::Keypair;
use libp2p_rs::core::{Multiaddr, PeerId, ProtocolId};

use libp2p_rs::swarm::{Control as SwarmControl, Swarm, SwarmError};
use libp2p_rs::kad::Control as KadControl;
use libp2p_rs::mdns::control::Control as MdnsControl;
use libp2p_rs::floodsub::control::Control as FloodsubControl;
use bitswap::{Control as BitswapControl, BlockStore, Block};

use libp2p_rs::kad::store::MemoryStore;

use bitswap::Bitswap;

use libp2p_rs::swarm::identify::IdentifyConfig;
use libp2p_rs::swarm::ping::PingConfig;
use libp2p_rs::kad::kad::{KademliaConfig, Kademlia};
use libp2p_rs::floodsub::FloodsubConfig;
use libp2p_rs::floodsub::floodsub::FloodSub;
use libp2p_rs::{noise, yamux, mplex};
use libp2p_rs::core::upgrade::Selector;
use libp2p_rs::core::transport::upgrade::TransportUpgrade;
use libp2p_rs::tcp::TcpConfig;
use libp2p_rs::dns::DnsConfig;

use libp2p_rs::swarm::protocol_handler::ProtocolHandler;


/// Libp2p Network controllers.
pub struct Controls<Types: IpfsTypes> {
    repo: Arc<Repo<Types>>,

    swarm: SwarmControl,
    kad: KadControl,
    pubsub: FloodsubControl,
    // mdns: MdnsControl,


    //kad_subscriptions: SubscriptionRegistry<KadResult, String>,
    bitswap: BitswapControl,
}

impl<Types: IpfsTypes> Clone for Controls<Types> {
    fn clone(&self) -> Self {
        Self {
            repo: self.repo.clone(),
            swarm: self.swarm.clone(),
            kad: self.kad.clone(),
            pubsub: self.pubsub.clone(),
            bitswap: self.bitswap.clone()
        }
    }
}


/// Defines the configuration for an IPFS swarm.
pub struct SwarmOptions {
    /// The keypair for the PKI based identity of the local node.
    pub keypair: Keypair,
    /// The peers to connect to on startup.
    pub bootstrap: Vec<(Multiaddr, PeerId)>,
    /// Enables mdns for peer discovery and announcement when true.
    pub mdns: bool,
    /// Custom Kademlia protocol name, see [`IpfsOptions::kad_protocol`].
    pub kad_protocol: Option<String>,
}

impl From<&IpfsOptions> for SwarmOptions {
    fn from(options: &IpfsOptions) -> Self {
        let keypair = options.keypair.clone();
        let bootstrap = options.bootstrap.clone();
        let mdns = options.mdns;
        let kad_protocol = options.kad_protocol.clone();

        SwarmOptions {
            keypair,
            bootstrap,
            mdns,
            kad_protocol,
        }
    }
}

/// Creates a new IPFS swarm.
pub async fn create_controls<TIpfsTypes: IpfsTypes>(
    options: SwarmOptions,
    span: Span,
    repo: Arc<Repo<TIpfsTypes>>,
) -> Controls<TIpfsTypes> {
    // Set up an encrypted TCP transport over the Yamux or Mplex protocol.
    let xx_keypair = noise::Keypair::<noise::X25519Spec>::new()
        .into_authentic(&options.keypair)
        .unwrap();
    let sec = noise::NoiseConfig::xx(xx_keypair, options.keypair.clone());

    let mux = Selector::new(yamux::Config::new(), mplex::Config::new());
    let tu = TransportUpgrade::new(TcpConfig::new().nodelay(true), mux, sec);//.timeout(Duration::from_secs(20));

    // Make swarm
    let mut swarm = Swarm::new(options.keypair.public())
        .with_transport(Box::new(DnsConfig::new(tu)))
        .with_ping(PingConfig::new())
        .with_identify(IdentifyConfig::new(false));
    let swarm_control = swarm.control();

    log::info!("Swarm created, local-peer-id={:?}", swarm.local_peer_id());

    // build Kad
    let mut kad_config = KademliaConfig::default().with_query_timeout(Duration::from_secs(180));
    if let Some(protocol) = options.kad_protocol {
        fn string_to_static_str(s: String) -> &'static str {
            Box::leak(s.into_boxed_str())
        }
        let s = string_to_static_str(protocol);
        kad_config = kad_config.with_protocol_name(ProtocolId::from(s.as_bytes()));
    }

    let store = MemoryStore::new(swarm.local_peer_id().clone());
    let kad = Kademlia::with_config(swarm.local_peer_id().clone(), store, kad_config);

    let kad_control = kad.control();

    // update Swarm to support Kad and Routing
    swarm = swarm.with_protocol(Box::new(kad.handler())).with_routing(Box::new(kad.control()));

    let mut floodsub_config = FloodsubConfig::new(swarm.local_peer_id().clone());
    floodsub_config.subscribe_local_messages = true;

    let floodsub = FloodSub::new(floodsub_config);
    let floodsub_control = floodsub.control();

    // register floodsub into Swarm
    swarm = swarm.with_protocol(Box::new(floodsub.handler()));


    // bitswap
    struct TestRepo;
    impl BlockStore for TestRepo {
        fn get(&self, cid: &Cid) -> Result<Option<Block>, Box<dyn Error>> {
            Ok(None)
        }
    }

    let bitswap = Bitswap::new(Box::new(TestRepo));
    let bitswap_control = bitswap.control();
    let mut handler = bitswap.handler();

    // let a = handler.protocol_info();
    // handler.handle();

    // register bitswap into Swarm
    swarm = swarm.with_protocol(Box::new(handler));

        // To start Swarm/Kad/... main loops
    kad.start(swarm_control.clone());
    swarm.start();

    // kad_control.add_node(bootstrap_peer, vec![bootstrap_addr]).await;
    // kad_control.bootstrap().await;

    for (addr, peer_id) in options.bootstrap {
        // TODO: bootstrap kad ??
    }

    Controls {
        repo,
        swarm: swarm_control,
        kad: kad_control,
        pubsub: floodsub_control,
        bitswap: bitswap_control,
        //mdns: ()
    }
}

// struct SpannedExecutor(Span);
//
// impl libp2p::core::Executor for SpannedExecutor {
//     fn exec(
//         &self,
//         future: std::pin::Pin<Box<dyn std::future::Future<Output = ()> + 'static + Send>>,
//     ) {
//         use tracing_futures::Instrument;
//         tokio::task::spawn(future.instrument(self.0.clone()));
//     }
// }


impl<Types: IpfsTypes> Controls<Types> {

    pub async fn add_peer(&mut self, peer: PeerId, addr: Multiaddr) {
        self.kad.add_node(peer.clone(), vec![addr]).await;
        //self.swarm.add_peer(peer);

        // FIXME: the call below automatically performs a dial attempt
        // to the given peer; it is unsure that we want it done within
        // add_peer, especially since that peer might not belong to the
        // expected identify protocol
        //self.pubsub.add_node_to_partial_view(peer);
        // TODO self.bitswap.add_node_to_partial_view(peer);
    }

    pub async fn remove_peer(&mut self, peer: &PeerId) {
        self.kad.remove_node(peer.clone()).await;
        //self.swarm.remove_peer(&peer);
        //self.pubsub.remove_node_from_partial_view(&peer);
        // TODO self.bitswap.remove_peer(&peer);
    }

    pub fn addrs(&self) -> Vec<(PeerId, Vec<Multiaddr>)> {
        let peers = self.swarm.get_peers();
        let mut addrs = Vec::with_capacity(peers.len());

        for peer_id in peers.into_iter() {
            let peer_addrs = self.swarm.get_addrs(&peer_id).unwrap_or(vec![]);
            addrs.push((peer_id, peer_addrs));
        }
        addrs
    }

    // pub fn connections(&self) -> impl Iterator<Item = Connection> + '_ {
    //     self.swarm.connections()
    // }

    pub async fn connect(&mut self, addr: MultiaddrWithPeerId) -> Result<(), SwarmError> {
        self.swarm.connect_with_addrs(addr.peer_id, vec![addr.multiaddr.into()]).await
    }

    pub async fn disconnect(&mut self, addr: MultiaddrWithPeerId) -> Result<(), SwarmError> {
        self.swarm.disconnect(addr.peer_id).await
    }

    // FIXME: it would be best if get_providers is called only in case the already connected
    // peers don't have it
    pub async fn want_block(&mut self, cid: Cid) {
        let key = cid.hash().as_bytes().to_owned();
        self.kad.find_providers(key.into(), 1).await;
        self.bitswap.want_block(cid, 1).await;
    }

    pub async fn stop_providing_block(&mut self, cid: &Cid) {
        info!("Finished providing block {}", cid.to_string());
        //let hash = Multihash::from_bytes(cid.to_bytes()).unwrap();
        //self.kad.remove_providing(&hash);
    }

    // pub fn pubsub(&mut self) -> &mut Pubsub {
    //     &mut self.pubsub
    // }
    //
    // pub fn bitswap(&mut self) -> &mut Bitswap {
    //     &mut self.bitswap
    // }

    pub async fn bootstrap(&mut self) {
        self.kad.bootstrap().await;
    }

    pub fn swarm(&mut self) -> &mut SwarmControl {
        &mut self.swarm
    }

    pub fn kad(&mut self) -> &mut KadControl {
        &mut self.kad
    }

    pub fn pubsub(&mut self) -> &mut FloodsubControl {
        &mut self.pubsub
    }

    pub fn bitswap(&mut self) -> &mut BitswapControl {
        &mut self.bitswap
    }

    // pub fn get_bootstrappers(&self) -> Vec<Multiaddr> {
    //     self.swarm
    //         .bootstrappers
    //         .iter()
    //         .cloned()
    //         .map(|a| a.into())
    //         .collect()
    // }
    //
    // pub fn add_bootstrapper(
    //     &mut self,
    //     addr: MultiaddrWithPeerId,
    // ) -> Result<Multiaddr, anyhow::Error> {
    //     let ret = addr.clone().into();
    //     if self.swarm.bootstrappers.insert(addr.clone()) {
    //         let MultiaddrWithPeerId {
    //             multiaddr: ma,
    //             peer_id,
    //         } = addr;
    //         self.kad.add_address(&peer_id, ma.into());
    //         // the return value of add_address doesn't implement Debug
    //         trace!(peer_id=%peer_id, "tried to add a bootstrapper");
    //     }
    //     Ok(ret)
    // }
    //
    // pub fn remove_bootstrapper(
    //     &mut self,
    //     addr: MultiaddrWithPeerId,
    // ) -> Result<Multiaddr, anyhow::Error> {
    //     let ret = addr.clone().into();
    //     if self.swarm.bootstrappers.remove(&addr) {
    //         let peer_id = addr.peer_id;
    //         let prefix: Multiaddr = addr.multiaddr.into();
    //
    //         if let Some(e) = self.kad.remove_address(&peer_id, &prefix) {
    //             info!(peer_id=%peer_id, status=?e.status, "removed bootstrapper");
    //         } else {
    //             warn!(peer_id=%peer_id, "attempted to remove an unknown bootstrapper");
    //         }
    //     }
    //     Ok(ret)
    // }
    //
    // pub fn clear_bootstrappers(&mut self) -> Vec<Multiaddr> {
    //     let removed = self.swarm.bootstrappers.drain();
    //     let mut ret = Vec::with_capacity(removed.len());
    //
    //     for addr_with_peer_id in removed {
    //         let peer_id = &addr_with_peer_id.peer_id;
    //         let prefix: Multiaddr = addr_with_peer_id.multiaddr.clone().into();
    //
    //         if let Some(e) = self.kad.remove_address(peer_id, &prefix) {
    //             info!(peer_id=%peer_id, status=?e.status, "cleared bootstrapper");
    //             ret.push(addr_with_peer_id.into());
    //         } else {
    //             error!(peer_id=%peer_id, "attempted to clear an unknown bootstrapper");
    //         }
    //     }
    //
    //     ret
    // }
    //
    // pub fn restore_bootstrappers(&mut self) -> Result<Vec<Multiaddr>, anyhow::Error> {
    //     let mut ret = Vec::new();
    //
    //     for addr in BOOTSTRAP_NODES {
    //         let addr = addr
    //             .parse::<MultiaddrWithPeerId>()
    //             .expect("see test bootstrap_nodes_are_multiaddr_with_peerid");
    //         if self.swarm.bootstrappers.insert(addr.clone()) {
    //             let MultiaddrWithPeerId {
    //                 multiaddr: ma,
    //                 peer_id,
    //             } = addr.clone();
    //
    //             // this is intentionally the multiaddr without peerid turned into plain multiaddr:
    //             // libp2p cannot dial addresses which include peerids.
    //             let ma: Multiaddr = ma.into();
    //
    //             // same as with add_bootstrapper: the return value from kad.add_address
    //             // doesn't implement Debug
    //             self.kad.add_address(&peer_id, ma.clone());
    //             trace!(peer_id=%peer_id, "tried to restore a bootstrapper");
    //
    //             // report with the peerid
    //             let reported: Multiaddr = addr.into();
    //             ret.push(reported);
    //         }
    //     }
    //
    //     Ok(ret)
    // }
}
