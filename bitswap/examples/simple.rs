use bitswap::Control as Bitswap_Control;
use bitswap::{Bitswap, Block, BlockStore};
use cid::Cid;
use libp2p_rs::core::identity::Keypair;
use libp2p_rs::core::transport::upgrade::TransportUpgrade;
use libp2p_rs::core::upgrade::Selector;
use libp2p_rs::core::{Multiaddr, PeerId};
use libp2p_rs::mplex;
use libp2p_rs::noise::{Keypair as NKeypair, NoiseConfig, X25519Spec};
use libp2p_rs::runtime::task;
use libp2p_rs::secio;
use libp2p_rs::swarm::identify::IdentifyConfig;
use libp2p_rs::swarm::Control as Swarm_Control;
use libp2p_rs::swarm::Swarm;
use libp2p_rs::tcp::TcpConfig;
use libp2p_rs::yamux;
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
        swarm.connect_with_addrs(peer, vec![addr]).await;

        // publish
        loop {
            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line);
            let x: &[_] = &['\r', '\n'];
            let msg = line.trim_end_matches(x);

            let cid = Cid::from_str(msg).expect("invalid cid");
            let rx = bitswap.want_block(cid.clone()).await;
            let block = rx.await.unwrap();
            log::info!("{}: {:?}", cid, block)
        }
    });
}

struct TestRepo;

impl BlockStore for TestRepo {
    fn get(&self, cid: &Cid) -> Result<Option<Block>, Box<dyn Error>> {
        // macrocan
        let cid = Cid::from_str("Qmf8CaZy6LeATsKGEKDWS6G6FZbaCxVG3sAH6nCXESdnVq").unwrap();
        let data: Vec<u8> = vec![
            10, 14, 8, 2, 18, 8, 109, 97, 99, 114, 111, 99, 97, 110, 24, 8,
        ];
        let block = Block::new(data.into_boxed_slice(), cid);
        Ok(Some(block))
    }
}

fn setup_swarm(keys: Keypair, listen_addr: Multiaddr) -> (Swarm_Control, Bitswap_Control) {
    let dh = NKeypair::<X25519Spec>::new().into_authentic(&keys).unwrap();

    let sec_noise = NoiseConfig::xx(dh, keys.clone());
    let sec_secio = secio::Config::new(keys.clone());
    let sec = Selector::new(sec_noise, sec_secio);

    let mux = Selector::new(yamux::Config::new(), mplex::Config::new());
    let tu = TransportUpgrade::new(TcpConfig::default(), mux, sec);

    let bitswap = Bitswap::new(Box::new(TestRepo));
    let handler = bitswap.handler();

    let mut swarm = Swarm::new(keys.public())
        .with_transport(Box::new(tu))
        .with_protocol(Box::new(handler))
        .with_identify(IdentifyConfig::new(false));
    let swarm_control = swarm.control();

    swarm.listen_on(vec![listen_addr]).expect("listen on");

    log::info!("Swarm created, local-peer-id={:?}", swarm.local_peer_id());

    // run bitswap message process main loop
    let bitswap_control = bitswap.control();
    bitswap.start(swarm_control.clone());

    swarm.start();

    (swarm_control, bitswap_control)
}
