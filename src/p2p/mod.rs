//! P2P handling for IPFS nodes.
use std::time::Duration;

use crate::repo::Repo;
use crate::{IpfsOptions, RepoTypes};

pub(crate) mod addr;
pub(crate) mod pubsub;
mod swarm;

pub use addr::{MultiaddrWithPeerId, MultiaddrWithoutPeerId};
pub use swarm::Connection;

use libp2p_rs::core::identity::Keypair;
use libp2p_rs::core::{Multiaddr, PeerId, ProtocolId, Transport};

use libp2p_rs::kad::Control as KadControl;
use libp2p_rs::swarm::{Control as SwarmControl, Swarm};
//use libp2p_rs::mdns::control::Control as MdnsControl;
use bitswap::Control as BitswapControl;
use libp2p_rs::floodsub::control::Control as FloodsubControl;

use libp2p_rs::kad::store::MemoryStore;

use bitswap::Bitswap;

use libp2p_rs::core::transport::upgrade::TransportUpgrade;
use libp2p_rs::core::upgrade::Selector;
use libp2p_rs::dns::DnsConfig;
use libp2p_rs::floodsub::floodsub::FloodSub;
use libp2p_rs::floodsub::FloodsubConfig;
use libp2p_rs::kad::kad::{Kademlia, KademliaConfig};
use libp2p_rs::swarm::identify::IdentifyConfig;
use libp2p_rs::swarm::ping::PingConfig;
use libp2p_rs::tcp::TcpConfig;
use libp2p_rs::{mplex, noise, secio, yamux};

/// Libp2p Network controllers.
pub struct Controls {
    swarm: SwarmControl,
    kad: KadControl,
    pubsub: FloodsubControl,
    bitswap: BitswapControl,
    // mdns: MdnsControl,
}

impl Clone for Controls {
    fn clone(&self) -> Self {
        Self {
            swarm: self.swarm.clone(),
            kad: self.kad.clone(),
            pubsub: self.pubsub.clone(),
            bitswap: self.bitswap.clone(),
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
    pub bootstrap: Vec<(PeerId, Multiaddr)>,
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

impl Controls {
    pub(crate) async fn build<T: RepoTypes>(repo: Repo<T>, options: SwarmOptions) -> Self {
        // start with security layer
        let sec_secio = secio::Config::new(options.keypair.clone());
        // Set up an encrypted TCP transport over the Yamux or Mplex protocol.
        let xx_keypair = noise::Keypair::<noise::X25519Spec>::new()
            .into_authentic(&options.keypair)
            .unwrap();
        let sec_noise = noise::NoiseConfig::xx(xx_keypair, options.keypair.clone());
        let sec = Selector::new(sec_noise, sec_secio);

        let mux = Selector::new(yamux::Config::new(), mplex::Config::new());
        let tu = TransportUpgrade::new(
            DnsConfig::new(
                TcpConfig::new()
                    .nodelay(true)
                    .outbound_timeout(Duration::from_secs(20)),
            ),
            mux,
            sec,
        );

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

        let store = MemoryStore::new(*swarm.local_peer_id());
        let kad = Kademlia::with_config(*swarm.local_peer_id(), store, kad_config);

        let mut kad_control = kad.control();

        // update Swarm to support Kad and Routing
        swarm = swarm
            .with_protocol(kad)
            .with_routing(Box::new(kad_control.clone()));

        let mut floodsub_config = FloodsubConfig::new(*swarm.local_peer_id());
        floodsub_config.subscribe_local_messages = true;

        let floodsub = FloodSub::new(floodsub_config);
        let floodsub_control = floodsub.control();

        // register floodsub into Swarm
        // swarm = swarm.with_protocol(floodsub);

        // bitswap
        let bitswap = Bitswap::new(repo, kad_control.clone());
        let bitswap_control = bitswap.control();

        // register bitswap into Swarm
        swarm = swarm.with_protocol(bitswap);

        // To start Swarm/Kad/... main loops
        swarm.start();

        // handle bootstrap nodes
        if !options.bootstrap.is_empty() {
            kad_control.bootstrap(options.bootstrap.clone()).await;
        }

        Controls {
            swarm: swarm_control,
            kad: kad_control,
            pubsub: floodsub_control,
            bitswap: bitswap_control,
            //mdns: ()
        }
    }
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

    pub fn addrs(&self) -> Vec<(PeerId, Vec<Multiaddr>)> {
        let peers = self.swarm.get_peers();
        let mut addrs = Vec::with_capacity(peers.len());

        for peer_id in peers.into_iter() {
            let peer_addrs = self.swarm.get_addrs(&peer_id).unwrap_or_default();
            addrs.push((peer_id, peer_addrs));
        }
        addrs
    }

    pub fn swarm_mut(&mut self) -> &mut SwarmControl {
        &mut self.swarm
    }
    pub fn kad_mut(&mut self) -> &mut KadControl {
        &mut self.kad
    }
    pub fn pubsub_mut(&mut self) -> &mut FloodsubControl {
        &mut self.pubsub
    }
    pub fn bitswap_mut(&mut self) -> &mut BitswapControl {
        &mut self.bitswap
    }

    pub fn swarm(&self) -> SwarmControl {
        self.swarm.clone()
    }
    pub fn kad(&self) -> KadControl {
        self.kad.clone()
    }
    pub fn pubsub(&self) -> FloodsubControl {
        self.pubsub.clone()
    }
    pub fn bitswap(&self) -> BitswapControl {
        self.bitswap.clone()
    }
}
