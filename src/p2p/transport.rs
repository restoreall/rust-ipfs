use std::io::{self, Error, ErrorKind};
use std::time::Duration;

use libp2p_rs::core::muxing;
use libp2p_rs::core::transport::upgrade;
use libp2p_rs::dns::DnsConfig;
use libp2p_rs::core::identity;
use libp2p_rs::core::{PeerId, Transport};
use libp2p_rs::mplex;
use libp2p_rs::yamux;
use libp2p_rs::noise;
use libp2p_rs::tcp::TcpConfig;
use libp2p_rs::core::upgrade::Selector;
use libp2p_rs::core::transport::upgrade::{TransportUpgrade, ITransportEx};

/// Builds the transport that serves as a common ground for all connections.
///
/// Set up an encrypted TCP transport over the Mplex protocol.
pub fn build_transport(keypair: identity::Keypair) -> io::Result<ITransportEx> {
}
