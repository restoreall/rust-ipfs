use async_trait::async_trait;
use bitswap::Control as Bitswap_Control;
use bitswap::{Bitswap, Block, BsBlockStore};
use cid::Cid;
use libp2p_rs::core::identity::Keypair;
use libp2p_rs::core::transport::upgrade::TransportUpgrade;
use libp2p_rs::core::{Multiaddr, PeerId};
use libp2p_rs::kad::kad::Kademlia;
use libp2p_rs::kad::store::MemoryStore;
use libp2p_rs::mplex;
use libp2p_rs::noise::{Keypair as NKeypair, NoiseConfig, X25519Spec};
use libp2p_rs::runtime::task;
use libp2p_rs::swarm::identify::IdentifyConfig;
use libp2p_rs::swarm::Control as Swarm_Control;
use libp2p_rs::swarm::Swarm;
use libp2p_rs::tcp::TcpConfig;
use std::convert::TryFrom;
use std::error::Error;
use std::str::FromStr;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    if std::env::args().len() == 3 {
        log::info!("Starting...");
        let a1 = std::env::args().nth(1).unwrap();
        let a2 = std::env::args().nth(2).unwrap();

        let peer = match PeerId::try_from(a1) {
            Ok(peer) => peer,
            Err(e) => {
                println!("bad peer id: {:?}", e);
                return;
            }
        };
        let addr = match Multiaddr::try_from(a2) {
            Ok(addr) => addr,
            Err(e) => {
                println!("bad multiaddr: {:?}", e);
                return;
            }
        };

        run(peer, addr);
    } else {
        println!("Usage: {} <address>", std::env::args().next().unwrap());
    }
}

fn run(peer: PeerId, addr: Multiaddr) {
    let key = Keypair::generate_ed25519_fixed();
    let listen_addr: Multiaddr = "/ip4/0.0.0.0/tcp/8086".parse().unwrap();
    let (mut swarm, mut bitswap) = setup_swarm(key, listen_addr);

    task::block_on(async {
        let _ = swarm.connect_with_addrs(peer, vec![addr]).await;

        // read cid from stdin, then get block via bitswap
        loop {
            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line);
            let x: &[_] = &['\r', '\n'];
            let msg = line.trim_end_matches(x);

            let cid = Cid::from_str(msg).expect("invalid cid");
            let res = bitswap.want_block(cid.clone(), 1).await;
            if let Ok(block) = res {
                log::info!("{}: {:?}", cid, block)
            }
        }
    });
}

#[derive(Clone)]
struct TestRepo;

#[async_trait]
impl BsBlockStore for TestRepo {
    async fn contains(&self, _cid: &Cid) -> Result<bool, Box<dyn Error>> {
        Ok(false)
    }

    async fn get(&self, _cid: &Cid) -> Result<Option<Block>, Box<dyn Error>> {
        Ok(None)
    }

    async fn put(&self, block: Block) -> Result<(Cid, bool), Box<dyn Error>> {
        Ok((block.cid, true))
    }

    async fn remove(&self, _cid: &Cid) -> Result<(), Box<dyn Error>> {
        Ok(())
    }
}

fn setup_swarm(keys: Keypair, listen_addr: Multiaddr) -> (Swarm_Control, Bitswap_Control) {
    let dh = NKeypair::<X25519Spec>::new().into_authentic(&keys).unwrap();
    let sec = NoiseConfig::xx(dh, keys.clone());
    let mux = mplex::Config::new();
    let tu = TransportUpgrade::new(TcpConfig::default(), mux, sec);

    let mut swarm = Swarm::new(keys.public())
        .with_transport(Box::new(tu))
        .with_identify(IdentifyConfig::new(false));

    // kad protocol
    let store = MemoryStore::new(*swarm.local_peer_id());
    let kad = Kademlia::new(*swarm.local_peer_id(), store);
    let kad_ctrl = kad.control();

    // bitswap protocol
    let bitswap = Bitswap::new(TestRepo, kad_ctrl.clone());
    let bitswap_control = bitswap.control();

    // register kad and bitswap
    swarm = swarm
        .with_protocol(kad)
        .with_routing(Box::new(kad_ctrl))
        .with_protocol(bitswap);
    let swarm_control = swarm.control();

    swarm.listen_on(vec![listen_addr]).expect("listen on");

    log::info!("Swarm created, local-peer-id={:?}", swarm.local_peer_id());

    swarm.start();

    (swarm_control, bitswap_control)
}
