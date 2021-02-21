//! P2P handling for IPFS nodes.
use std::time::Duration;

use crate::repo::Repo;
use crate::{IpfsOptions, IpfsTypes, Cid};

pub(crate) mod addr;
pub(crate) mod pubsub;
mod swarm;



pub use addr::{MultiaddrWithPeerId, MultiaddrWithoutPeerId};
pub use {swarm::Connection};

use libp2p_rs::core::identity::Keypair;
use libp2p_rs::core::{Multiaddr, PeerId, ProtocolId};

use libp2p_rs::swarm::{Control as SwarmControl, Swarm};
use libp2p_rs::kad::Control as KadControl;
//use libp2p_rs::mdns::control::Control as MdnsControl;
use libp2p_rs::floodsub::control::Control as FloodsubControl;
use bitswap::Control as BitswapControl;

use libp2p_rs::kad::store::MemoryStore;

use bitswap::Bitswap;

use libp2p_rs::swarm::identify::IdentifyConfig;
use libp2p_rs::swarm::ping::PingConfig;
use libp2p_rs::kad::kad::{KademliaConfig, Kademlia};
use libp2p_rs::floodsub::FloodsubConfig;
use libp2p_rs::floodsub::floodsub::FloodSub;
use libp2p_rs::{noise, yamux, mplex, secio};
use libp2p_rs::core::upgrade::Selector;
use libp2p_rs::core::transport::upgrade::TransportUpgrade;
use libp2p_rs::tcp::TcpConfig;
//use libp2p_rs::dns::DnsConfig;

/// Libp2p Network controllers.
pub struct Controls<Types: IpfsTypes> {
    repo: Repo<Types>,

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
    /// Bound listening addresses; by default the node will not listen on any address.
    pub listening_addrs: Vec<Multiaddr>,
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
        let listening_addrs = options.listening_addrs.clone();
        let bootstrap = options.bootstrap.clone();
        let mdns = options.mdns;
        let kad_protocol = options.kad_protocol.clone();

        SwarmOptions {
            keypair,
            listening_addrs,
            bootstrap,
            mdns,
            kad_protocol,
        }
    }
}

/// Creates a new IPFS swarm.
pub async fn create_controls<TIpfsTypes: IpfsTypes>(
    options: SwarmOptions,
    repo: Repo<TIpfsTypes>,
) -> Controls<TIpfsTypes> {
    let sec_secio = secio::Config::new(options.keypair.clone());
    // Set up an encrypted TCP transport over the Yamux or Mplex protocol.
    let xx_keypair = noise::Keypair::<noise::X25519Spec>::new()
        .into_authentic(&options.keypair)
        .unwrap();
    let sec_noise = noise::NoiseConfig::xx(xx_keypair, options.keypair.clone());
    let sec = Selector::new(sec_noise, sec_secio);

    // FIXME: timeout & DnsConfig
    let mux = Selector::new(yamux::Config::new(), mplex::Config::new());
    let tu = TransportUpgrade::new(TcpConfig::new().nodelay(true), mux, sec);//.timeout(Duration::from_secs(20));

    // Make swarm
    let mut swarm = Swarm::new(options.keypair.public())
        .with_transport(Box::new(tu))
        .with_ping(PingConfig::new())
        .with_identify(IdentifyConfig::new(false));

    swarm.listen_on(options.listening_addrs).unwrap();

    let swarm_control = swarm.control();

    log::info!("Swarm created, local-peer-id={:?}", swarm.local_peer_id());

    // build Kad
    let mut kad_config = KademliaConfig::default().with_query_timeout(Duration::from_secs(90));
    if let Some(protocol) = options.kad_protocol {
        fn string_to_static_str(s: String) -> &'static str {
            Box::leak(s.into_boxed_str())
        }
        let s = string_to_static_str(protocol);
        kad_config = kad_config.with_protocol_name(ProtocolId::from(s.as_bytes()));
    }

    let store = MemoryStore::new(swarm.local_peer_id().clone());
    let kad = Kademlia::with_config(swarm.local_peer_id().clone(), store, kad_config);

    let mut kad_control = kad.control();

    // update Swarm to support Kad and Routing
    swarm = swarm.with_protocol(Box::new(kad.handler())).with_routing(Box::new(kad.control()));

    let mut floodsub_config = FloodsubConfig::new(swarm.local_peer_id().clone());
    floodsub_config.subscribe_local_messages = true;

    let floodsub = FloodSub::new(floodsub_config);
    let floodsub_control = floodsub.control();

    // register floodsub into Swarm
    swarm = swarm.with_protocol(Box::new(floodsub.handler()));

    // bitswap
    let bitswap = Bitswap::new(repo.clone());
    let bitswap_control = bitswap.control();

    // register bitswap into Swarm
    swarm = swarm.with_protocol(Box::new(bitswap.handler()));

    // To start Swarm/Kad/... main loops
    swarm.start();
    kad.start(swarm_control.clone());
    floodsub.start(swarm_control.clone());
    bitswap.start(swarm_control.clone());

    // handle bootstrap nodes
    for (addr, peer_id) in options.bootstrap {
        kad_control.add_node(peer_id, vec![addr]).await;
    }
    kad_control.bootstrap().await;

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
    //
    // pub async fn add_peer(&mut self, peer: PeerId, addr: Multiaddr) {
    //     self.kad.add_node(peer.clone(), vec![addr]).await;
    //     //self.swarm.add_peer(peer);
    //
    //     // FIXME: the call below automatically performs a dial attempt
    //     // to the given peer; it is unsure that we want it done within
    //     // add_peer, especially since that peer might not belong to the
    //     // expected identify protocol
    //     //self.pubsub.add_node_to_partial_view(peer);
    //     // TODO self.bitswap.add_node_to_partial_view(peer);
    // }
    //
    // pub async fn remove_peer(&mut self, peer: &PeerId) {
    //     self.kad.remove_node(peer.clone()).await;
    //     //self.swarm.remove_peer(&peer);
    //     //self.pubsub.remove_node_from_partial_view(&peer);
    //     // TODO self.bitswap.remove_peer(&peer);
    // }

    // FIXME: it would be best if get_providers is called only in case the already connected
    // peers don't have it
    pub async fn want_block(&mut self, cid: Cid) {
        let _ = self.kad.find_providers(cid.clone().into(), 1).await;
        let _ = self.bitswap.want_block(cid, 1).await;
    }

    pub async fn stop_providing_block(&mut self, cid: &Cid) {
        info!("Finished providing block {}", cid.to_string());
        //let hash = Multihash::from_bytes(cid.to_bytes()).unwrap();
        //self.kad.remove_providing(&hash);
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
}
