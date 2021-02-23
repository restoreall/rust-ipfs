//! IPFS node implementation
//!
//! [Ipfs](https://ipfs.io) is a peer-to-peer system with content addressed functionality. The main
//! entry point for users of this crate is the [`Ipfs`] facade, which allows access to most of the
//! implemented functionality.
//!
//! This crate passes a lot of the [interface-ipfs-core] test suite; most of that functionality is
//! in `ipfs-http` crate. The crate has some interoperability with the [go-ipfs] and [js-ipfs]
//! implementations.
//!
//! `ipfs` is an early alpha level crate: APIs and their implementation are subject to change in
//! any upcoming release at least for now. The aim of the crate is to become a library-first
//! production ready implementation of an Ipfs node.
//!
//! [interface-ipfs-core]: https://www.npmjs.com/package/interface-ipfs-core
//! [go-ipfs]: https://github.com/ipfs/go-ipfs/
//! [js-ipfs]: https://github.com/ipfs/js-ipfs/
// We are not done yet, but uncommenting this makes it easier to hunt down for missing docs.
//#![deny(missing_docs)]
//
// This isn't recognized in stable yet, but we should disregard any nags on these to keep making
// the docs better.
//#![allow(private_intra_doc_links)]

pub mod config;
pub mod dag;
pub mod error;
#[macro_use]
pub mod ipld;
pub mod cli;
pub mod ipns;
pub mod p2p;
pub mod path;
pub mod refs;
pub mod repo;
pub mod unixfs;

mod exchange;

#[macro_use]
extern crate tracing;

use anyhow::anyhow;
use cid::Codec;
use futures::{
    channel::{
        mpsc::{Receiver},
    },
    stream::Stream,
};
use tracing::Span;
use tracing_futures::Instrument;

use std::{
    borrow::Borrow,
    collections::HashSet,
    env, fmt,
    future::Future,
    ops::{Deref, DerefMut, Range},
    path::PathBuf,
    sync::atomic::Ordering,
};

use self::{
    dag::IpldDag,
    ipns::Ipns,
    p2p::{create_controls, SwarmOptions},
    repo::{create_repo, Repo, RepoEvent, RepoOptions},
};

pub use self::{
    error::Error,
    ipld::Ipld,
    p2p::{
        pubsub::PubsubMessage,
        Connection, MultiaddrWithPeerId, MultiaddrWithoutPeerId,
    },
    path::IpfsPath,
    repo::{PinKind, PinMode, RepoTypes},
};
pub use cid::Cid;
pub use bitswap::Block;
pub use bitswap::BsBlockStore;

pub use libp2p_rs::{
    core::{
        multiaddr::Protocol, Multiaddr, PeerId, PublicKey, identity::Keypair

    },
    kad::record::Key,
};

/// Represents the configuration of the Ipfs node, its backing blockstore and datastore.
pub trait IpfsTypes: RepoTypes {}

impl<T: RepoTypes> IpfsTypes for T {}

/// Default node configuration, currently with persistent block store and data store for pins.
#[derive(Debug)]
pub struct Types;
impl RepoTypes for Types {
    type TBlockStore = repo::fs::FsBlockStore;
    #[cfg(feature = "sled_data_store")]
    type TDataStore = repo::kv::KvDataStore;
    #[cfg(not(feature = "sled_data_store"))]
    type TDataStore = repo::fs::FsDataStore;
    type TLock = repo::fs::FsLock;
}

/// In-memory testing configuration used in tests.
#[derive(Debug)]
pub struct TestTypes;
impl RepoTypes for TestTypes {
    type TBlockStore = repo::mem::MemBlockStore;
    type TDataStore = repo::mem::MemDataStore;
    type TLock = repo::mem::MemLock;
}

/// Ipfs node options used to configure the node to be created with [`UninitializedIpfs`].
#[derive(Clone)]
pub struct IpfsOptions {
    /// The path of the ipfs repo (blockstore and datastore).
    ///
    /// This is always required but can be any path with in-memory backends. The filesystem backend
    /// creates a directory structure alike but not compatible to other ipfs implementations.
    ///
    /// # Incompatiblity and interop warning
    ///
    /// It is **not** recommended to set this to IPFS_PATH without first at least backing up your
    /// existing repository.
    pub ipfs_path: PathBuf,

    /// The keypair used with libp2p, the identity of the node.
    pub keypair: Keypair,

    /// Nodes used as bootstrap peers.
    pub bootstrap: Vec<(Multiaddr, PeerId)>,

    /// Enables mdns for peer discovery and announcement when true.
    pub mdns: bool,

    /// Custom Kademlia protocol name. When set to `None`, the global DHT name is used instead of
    /// the LAN dht name.
    ///
    /// The name given here is passed to [`libp2p_kad::KademliaConfig::set_protocol_name`].
    ///
    /// [`libp2p_kad::KademliaConfig::set_protocol_name`]: https://docs.rs/libp2p-kad/*/libp2p_kad/struct.KademliaConfig.html##method.set_protocol_name
    pub kad_protocol: Option<String>,

    /// Bound listening addresses; by default the node will not listen on any address.
    pub listening_addrs: Vec<Multiaddr>,

    /// The span for tracing purposes, `None` value is converted to `tracing::trace_span!("ipfs")`.
    ///
    /// All futures returned by `Ipfs`, background task actions and swarm actions are instrumented
    /// with this span or spans referring to this as their parent. Setting this other than `None`
    /// default is useful when running multiple nodes.
    pub span: Option<Span>,
}

impl fmt::Debug for IpfsOptions {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        // needed since libp2p::identity::Keypair does not have a Debug impl, and the IpfsOptions
        // is a struct with all public fields, don't enforce users to use this wrapper.
        fmt.debug_struct("IpfsOptions")
            .field("ipfs_path", &self.ipfs_path)
            .field("bootstrap", &self.bootstrap)
            .field("keypair", &DebuggableKeypair(&self.keypair))
            .field("mdns", &self.mdns)
            .field("kad_protocol", &self.kad_protocol)
            .field("listening_addrs", &self.listening_addrs)
            .field("span", &self.span)
            .finish()
    }
}

impl IpfsOptions {
    /// Creates an in-memory store backed configuration useful for any testing purposes.
    ///
    /// Also used from examples.
    pub fn inmemory_with_generated_keys() -> Self {
        Self {
            ipfs_path: env::temp_dir(),
            keypair: Keypair::generate_ed25519(),
            mdns: Default::default(),
            bootstrap: Default::default(),
            // default to lan kad for go-ipfs use in tests
            kad_protocol: Some("/ipfs/kad/1.0.0".to_owned()),
            listening_addrs: vec!["/ip4/127.0.0.1/tcp/0".parse().unwrap()],
            span: None,
        }
    }

    // If IPFS_PATH is exists, use disk storage.
    // Otherwise, use memory.
    pub fn disk_with_generated_keys() -> Self {
        match std::env::var("IPFS_PATH") {
            Ok(path) => {
                Self {
                    ipfs_path: PathBuf::from(path),
                    keypair: Keypair::generate_ed25519(),
                    mdns: Default::default(),
                    bootstrap: Default::default(),
                    // default to lan kad for go-ipfs use in tests
                    kad_protocol: Some("/ipfs/lan/kad/1.0.0".to_owned()),
                    listening_addrs: vec!["/ip4/127.0.0.1/tcp/0".parse().unwrap()],
                    span: None,
                }
            }
            Err(_e) => IpfsOptions::inmemory_with_generated_keys(),
        }
    }
}

/// Workaround for libp2p::identity::Keypair missing a Debug impl, works with references and owned
/// keypairs.
#[derive(Clone)]
struct DebuggableKeypair<I: Borrow<Keypair>>(I);

impl<I: Borrow<Keypair>> fmt::Debug for DebuggableKeypair<I> {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self.get_ref() {
            Keypair::Ed25519(_) => "Ed25519",
            Keypair::Rsa(_) => "Rsa",
            Keypair::Secp256k1(_) => "Secp256k1",
        };

        write!(fmt, "Keypair::{}", kind)
    }
}

impl<I: Borrow<Keypair>> DebuggableKeypair<I> {
    fn get_ref(&self) -> &Keypair {
        self.0.borrow()
    }
}

/// The facade for the Ipfs node.
///
/// The facade has most of the functionality either directly as a method or the functionality can
/// be implemented using the provided methods. For more information, see examples or the HTTP
/// endpoint implementations in `ipfs-http`.
///
/// The facade is created through [`UninitializedIpfs`] which is configured with [`IpfsOptions`].
pub struct Ipfs<Types: IpfsTypes> {
    span: Span,
    repo: Repo<Types>,
    keys: DebuggableKeypair<Keypair>,
    controls: Controls<Types>,
}

impl<Types: IpfsTypes> Clone for Ipfs<Types> {
    fn clone(&self) -> Self {
        Ipfs {
            span: self.span.clone(),
            repo: self.repo.clone(),
            keys: self.keys.clone(),
            controls: self.controls.clone(),
        }
    }
}

//
// /// Events used internally to communicate with the swarm, which is executed in the the background
// /// task.
// #[derive(Debug)]
// enum IpfsEvent {
//     /// Connect
//     Connect(
//         MultiaddrWithPeerId,
//         OneshotSender<Option<SubscriptionFuture<(), String>>>,
//     ),
//     /// Addresses
//     Addresses(Channel<Vec<(PeerId, Vec<Multiaddr>)>>),
//     /// Local addresses
//     Listeners(Channel<Vec<Multiaddr>>),
//     /// Connections
//     Connections(Channel<Vec<Connection>>),
//     /// Disconnect
//     Disconnect(MultiaddrWithPeerId, Channel<()>),
//     /// Request background task to return the listened and external addresses
//     GetAddresses(OneshotSender<Vec<Multiaddr>>),
//     PubsubSubscribe(String, OneshotSender<Option<SubscriptionStream>>),
//     PubsubUnsubscribe(String, OneshotSender<bool>),
//     PubsubPublish(String, Vec<u8>, OneshotSender<()>),
//     PubsubPeers(Option<String>, OneshotSender<Vec<PeerId>>),
//     PubsubSubscribed(OneshotSender<Vec<String>>),
//     WantList(
//         Option<PeerId>,
//         OneshotSender<Vec<(Cid, bitswap::Priority)>>,
//     ),
//     BitswapStats(OneshotSender<BitswapStats>),
//     AddListeningAddress(Multiaddr, Channel<Multiaddr>),
//     RemoveListeningAddress(Multiaddr, Channel<()>),
//     Bootstrap(Channel<SubscriptionFuture<KadResult, String>>),
//     AddPeer(PeerId, Multiaddr),
//     GetClosestPeers(PeerId, OneshotSender<SubscriptionFuture<KadResult, String>>),
//     GetBitswapPeers(OneshotSender<Vec<PeerId>>),
//     FindPeer(
//         PeerId,
//         bool,
//         OneshotSender<Either<Vec<Multiaddr>, SubscriptionFuture<KadResult, String>>>,
//     ),
//     GetProviders(Cid, OneshotSender<SubscriptionFuture<KadResult, String>>),
//     Provide(Cid, Channel<SubscriptionFuture<KadResult, String>>),
//     DhtGet(
//         Key,
//         Quorum,
//         OneshotSender<SubscriptionFuture<KadResult, String>>,
//     ),
//     DhtPut(
//         Key,
//         Vec<u8>,
//         Quorum,
//         Channel<SubscriptionFuture<KadResult, String>>,
//     ),
//     GetBootstrappers(OneshotSender<Vec<Multiaddr>>),
//     AddBootstrapper(MultiaddrWithPeerId, Channel<Multiaddr>),
//     RemoveBootstrapper(MultiaddrWithPeerId, Channel<Multiaddr>),
//     ClearBootstrappers(OneshotSender<Vec<Multiaddr>>),
//     RestoreBootstrappers(Channel<Vec<Multiaddr>>),
//     Exit,
// }

/// Configured Ipfs which can only be started.
pub struct UninitializedIpfs<Types: IpfsTypes> {
    repo: Repo<Types>,
    keys: Keypair,
    options: IpfsOptions,
    repo_events: Receiver<RepoEvent>,
}

impl<Types: IpfsTypes> UninitializedIpfs<Types> {
    /// Configures a new UninitializedIpfs with from the given options and optionally a span.
    /// If the span is not given, it is defaulted to `tracing::trace_span!("ipfs")`.
    ///
    /// The span is attached to all operations called on the later created `Ipfs` along with all
    /// operations done in the background task as well as tasks spawned by the underlying
    /// `libp2p::Swarm`.
    pub fn new(options: IpfsOptions) -> Self {
        let repo_options = RepoOptions::from(&options);
        let (repo, repo_events) = create_repo(repo_options);
        let keys = options.keypair.clone();

        UninitializedIpfs {
            repo,
            keys,
            options,
            repo_events,
        }
    }

    /// Initialize the ipfs node. The returned `Ipfs` value is cloneable, send and sync, and the
    /// future should be spawned on a executor as soon as possible.
    ///
    /// The future returned from this method should not need
    /// (instrumenting)[`tracing_futures::Instrument::instrument`] as the [`IpfsOptions::span`]
    /// will be used as parent span for all of the awaited and created futures.
    pub async fn start(self) -> Result<(Ipfs<Types>, impl Future<Output = ()>), Error> {
        use futures::stream::StreamExt;

        let UninitializedIpfs {
            repo,
            keys,
            mut repo_events,
            mut options,
        } = self;

        let root_span = options
            .span
            .take()
            // not sure what would be the best practice with tracing and spans
            .unwrap_or_else(|| tracing::trace_span!(parent: &Span::current(), "ipfs"));

        // the "current" span which is not entered but the awaited futures are instrumented with it
        let init_span = tracing::trace_span!(parent: &root_span, "init");

        // stored in the Ipfs, instrumenting every method call
        let facade_span = tracing::trace_span!("facade");

        // instruments the Ipfs main loop, the background task.
        let ipfs_span = tracing::trace_span!(parent: &root_span, "ipfs");

        repo.init().instrument(init_span.clone()).await?;

        // FIXME: mutating options above is an unfortunate side-effect of this call, which could be
        // reordered for less error prone code.
        let swarm_options = SwarmOptions::from(&options);
        let mut controls = create_controls(swarm_options, repo.clone())
            .instrument(tracing::trace_span!(parent: &init_span, "swarm"))
            .await;

        let ipfs = Ipfs {
            span: facade_span,
            repo,
            keys: DebuggableKeypair(keys),
            controls: controls.clone(),
        };

        let mut bitswap = controls.bitswap().clone();
        let fut = async move {
            loop {
                match repo_events.next().await {
                    Some(evt) => {
                        match evt {
                            RepoEvent::WantBlock(cid, reply) => {
                                let b = bitswap.want_block(cid, 1).await.map_err(Error::from);
                                let _ = reply.send(b);
                            },
                            RepoEvent::UnwantBlock(cid) => {
                                let _ = bitswap.cancel_block(cid).await;
                            },
                            RepoEvent::NewBlock(cid, ret) => {
                                let _ = bitswap.has_block(cid).await;
                                let _ = ret.send(Ok(()));

                                // TODO: consider if cancel is applicable in cases where we provide the
                                // associated Block ourselves
                                //self.controls.bitswap().cancel_block(&cid);
                                // currently disabled; see https://github.com/rs-ipfs/rust-ipfs/pull/281#discussion_r465583345
                                // for details regarding the concerns about enabling this functionality as-is
                                // let _ = ret.send(Ok(()));
                                // if false {
                                //     let _ = ret.send(self.swarm.start_providing(cid));
                                // } else {
                                //     let _ = ret.send(Err(anyhow!("not actively providing blocks yet")));
                                // }
                            }
                            RepoEvent::RemovedBlock(_cid) => {
                                //self.swarm.stop_providing_block(&cid)
                            },
                        }
                    }
                    None => {
                        log::warn!("we are closed. exiting...");
                        return;
                    }
                }
            }
        };

        // TODO:
        // for addr in listening_addrs.into_iter() {
        //     fut.start_add_listener_address(addr, None);
        // }

        Ok((ipfs, fut.instrument(ipfs_span)))
    }
}

impl<Types: IpfsTypes> Ipfs<Types> {
    /// Returns the controls in IPFS.
    pub fn controls(&self) -> Controls<Types> { self.controls.clone() }

    /// Return an [`IpldDag`] for DAG operations
    pub fn dag(&self) -> IpldDag<Types> {
        IpldDag::new(self.clone())
    }

    fn ipns(&self) -> Ipns<Types> {
        Ipns::new(self.clone())
    }

    /// Puts a block into the ipfs repo.
    ///
    /// # Forget safety
    ///
    /// Forgetting the returned future will not result in memory unsafety, but it can
    /// deadlock other tasks.
    pub async fn put_block(&self, block: Block) -> Result<Cid, Error> {
        self.repo
            .put_block(block)
            .instrument(self.span.clone())
            .await
            .map(|(cid, _put_status)| cid)
    }

    /// Retrieves a block from the local blockstore, or starts fetching from the network or join an
    /// already started fetch.
    pub async fn get_block(&self, cid: &Cid) -> Result<Block, Error> {
        self.repo.get_block(cid).instrument(self.span.clone()).await
    }

    /// Remove block from the ipfs repo. A pinned block cannot be removed.
    pub async fn remove_block(&self, cid: Cid) -> Result<Cid, Error> {
        self.repo
            .remove_block(&cid)
            .instrument(self.span.clone())
            .await
    }

    /// Pins a given Cid recursively or directly (non-recursively).
    ///
    /// Pins on a block are additive in sense that a previously directly (non-recursively) pinned
    /// can be made recursive, but removing the recursive pin on the block removes also the direct
    /// pin as well.
    ///
    /// Pinning a Cid recursively (for supported dag-protobuf and dag-cbor) will walk its
    /// references and pin the references indirectly. When a Cid is pinned indirectly it will keep
    /// its previous direct or recursive pin and be indirect in addition.
    ///
    /// Recursively pinned Cids cannot be re-pinned non-recursively but non-recursively pinned Cids
    /// can be "upgraded to" being recursively pinned.
    ///
    /// # Crash unsafety
    ///
    /// If a recursive `insert_pin` operation is interrupted because of a crash or the crash
    /// prevents from synchronizing the data store to disk, this will leave the system in an inconsistent
    /// state. The remedy is to re-pin recursive pins.
    pub async fn insert_pin(&self, cid: &Cid, recursive: bool) -> Result<(), Error> {
        use futures::stream::{StreamExt, TryStreamExt};
        let span = debug_span!(parent: &self.span, "insert_pin", cid = %cid, recursive);
        let refs_span = debug_span!(parent: &span, "insert_pin refs");

        async move {
            // this needs to download everything but /pin/ls does not
            let Block { data, .. } = self.repo.get_block(cid).await?;

            if !recursive {
                self.repo.insert_direct_pin(cid).await
            } else {
                let ipld = crate::ipld::decode_ipld(&cid, &data)?;

                let st = crate::refs::IpldRefs::default()
                    .with_only_unique()
                    .refs_of_resolved(self, vec![(cid.clone(), ipld.clone())].into_iter())
                    .map_ok(|crate::refs::Edge { destination, .. }| destination)
                    .into_stream()
                    .instrument(refs_span)
                    .boxed();

                self.repo.insert_recursive_pin(cid, st).await
            }
        }
        .instrument(span)
        .await
    }

    /// Unpins a given Cid recursively or only directly.
    ///
    /// Recursively unpinning a previously only directly pinned Cid will remove the direct pin.
    ///
    /// Unpinning an indirectly pinned Cid is not possible other than through its recursively
    /// pinned tree roots.
    pub async fn remove_pin(&self, cid: &Cid, recursive: bool) -> Result<(), Error> {
        use futures::stream::{StreamExt, TryStreamExt};
        let span = debug_span!(parent: &self.span, "remove_pin", cid = %cid, recursive);
        async move {
            if !recursive {
                self.repo.remove_direct_pin(cid).await
            } else {
                // start walking refs of the root after loading it

                let Block { data, .. } = match self.repo.get_block_now(&cid).await? {
                    Some(b) => b,
                    None => {
                        return Err(anyhow::anyhow!("pinned root not found: {}", cid));
                    }
                };

                let ipld = crate::ipld::decode_ipld(&cid, &data)?;
                let st = crate::refs::IpldRefs::default()
                    .with_only_unique()
                    .with_existing_blocks()
                    .refs_of_resolved(
                        self.to_owned(),
                        vec![(cid.clone(), ipld.clone())].into_iter(),
                    )
                    .map_ok(|crate::refs::Edge { destination, .. }| destination)
                    .into_stream()
                    .boxed();

                self.repo.remove_recursive_pin(cid, st).await
            }
        }
        .instrument(span)
        .await
    }

    /// Checks whether a given block is pinned.
    ///
    /// Returns true if the block is pinned, false if not. See Crash unsafety notes for the false
    /// response.
    ///
    /// # Crash unsafety
    ///
    /// Cannot currently detect partially written recursive pins. Those can happen if
    /// `Ipfs::insert_pin(cid, true)` is interrupted by a crash for example.
    ///
    /// Works correctly only under no-crash situations. Workaround for hitting a crash is to re-pin
    /// any existing recursive pins.
    ///
    // TODO: This operation could be provided as a `Ipfs::fix_pins()`.
    pub async fn is_pinned(&self, cid: &Cid) -> Result<bool, Error> {
        let span = debug_span!(parent: &self.span, "is_pinned", cid = %cid);
        self.repo.is_pinned(cid).instrument(span).await
    }

    /// Lists all pins, or the specific kind thereof.
    ///
    /// # Crash unsafety
    ///
    /// Does not currently recover from partial recursive pin insertions.
    pub async fn list_pins(
        &self,
        filter: Option<PinMode>,
    ) -> futures::stream::BoxStream<'static, Result<(Cid, PinMode), Error>> {
        let span = debug_span!(parent: &self.span, "list_pins", ?filter);
        self.repo.list_pins(filter).instrument(span).await
    }

    /// Read specific pins. When `requirement` is `Some`, all pins are required to be of the given
    /// [`PinMode`].
    ///
    /// # Crash unsafety
    ///
    /// Does not currently recover from partial recursive pin insertions.
    pub async fn query_pins(
        &self,
        cids: Vec<Cid>,
        requirement: Option<PinMode>,
    ) -> Result<Vec<(Cid, PinKind<Cid>)>, Error> {
        let span = debug_span!(parent: &self.span, "query_pins", ids = cids.len(), ?requirement);
        self.repo
            .query_pins(cids, requirement)
            .instrument(span)
            .await
    }

    /// Puts an ipld node into the ipfs repo using `dag-cbor` codec and Sha2_256 hash.
    ///
    /// Returns Cid version 1 for the document
    pub async fn put_dag(&self, ipld: Ipld) -> Result<Cid, Error> {
        self.dag()
            .put(ipld, Codec::DagCBOR)
            .instrument(self.span.clone())
            .await
    }

    /// Gets an ipld node from the ipfs, fetching the block if necessary.
    ///
    /// See [`IpldDag::get`] for more information.
    pub async fn get_dag(&self, path: IpfsPath) -> Result<Ipld, Error> {
        self.dag()
            .get(path)
            .instrument(self.span.clone())
            .await
            .map_err(Error::new)
    }

    /// Creates a stream which will yield the bytes of an UnixFS file from the root Cid, with the
    /// optional file byte range. If the range is specified and is outside of the file, the stream
    /// will end without producing any bytes.
    ///
    /// To create an owned version of the stream, please use `ipfs::unixfs::cat` directly.
    pub async fn cat_unixfs(
        &self,
        starting_point: impl Into<unixfs::StartingPoint>,
        range: Option<Range<u64>>,
    ) -> Result<
        impl Stream<Item = Result<Vec<u8>, unixfs::TraversalFailed>> + Send + '_,
        unixfs::TraversalFailed,
    > {
        // convert early not to worry about the lifetime of parameter
        let starting_point = starting_point.into();
        unixfs::cat(self, starting_point, range)
            .instrument(self.span.clone())
            .await
    }

    /// Resolves a ipns path to an ipld path; currently only supports dnslink resolution.
    pub async fn resolve_ipns(&self, path: &IpfsPath, recursive: bool) -> Result<IpfsPath, Error> {
        async move {
            let ipns = self.ipns();
            let mut resolved = ipns.resolve(path).await;

            if recursive {
                let mut seen = HashSet::with_capacity(1);
                while let Ok(ref res) = resolved {
                    if !seen.insert(res.clone()) {
                        break;
                    }
                    resolved = ipns.resolve(&res).await;
                }

                resolved
            } else {
                resolved
            }
        }
        .instrument(self.span.clone())
        .await
    }

    /// Bootstraps the Kad-DHT.
    ///
    /// Assumes the bootstrap nodes have been added to the routing table already.
    /// Check [SwarmOptions::bootstrap] for details.
    pub async fn bootstrap(&mut self) {
        self.controls.kad().bootstrap().await;
    }

    // pub fn connections(&self) -> impl Iterator<Item = Connection> + '_ {
    //     self.swarm.connections()
    // }

    /// Connects to the peer at the given Multiaddress.
    ///
    /// Accepts only multiaddresses with the PeerId to authenticate the connection.
    ///
    /// Returns a future which will complete when the connection has been successfully made or
    /// failed for whatever reason.
    pub async fn connect(&mut self, target: MultiaddrWithPeerId) -> Result<(), Error> {
        self.controls.swarm().connect_with_addrs(target.peer_id, vec![target.multiaddr.into()])
            .instrument(self.span.clone())
            .await
            .map_err(Error::from)
    }

    /// Disconnects a given peer.
    ///
    /// At the moment the peer is disconnected by temporarily banning the peer and unbanning it
    /// right after. This should always disconnect all connections to the peer.
    pub async fn disconnect(&mut self, target: MultiaddrWithPeerId) -> Result<(), Error> {
        self.controls.swarm().disconnect(target.peer_id)
            .instrument(self.span.clone())
            .await
            .map_err(Error::from)
    }

    /// Returns known peer addresses
    pub async fn addrs(&self) -> Result<Vec<(PeerId, Vec<Multiaddr>)>, Error> {
        Ok(self.controls.addrs())
    }

    /// Returns local listening addresses
    pub async fn addrs_local(&mut self) -> Result<Vec<Multiaddr>, Error> {
        self.controls.swarm().self_addrs()
            .instrument(self.span.clone())
            .await
            .map_err(Error::from)
    }

    /// Returns the connected peers - connections
    pub async fn peers(&mut self) -> Result<Vec<Connection>, Error> {
        let connections = self.controls.swarm().dump_connections(None)
            .instrument(self.span.clone())
            .await?;

        // TODO: rtt
        let cc = connections
            .into_iter()
            .map(|c| {
                let addr = MultiaddrWithoutPeerId::try_from(c.info.ra)
                            .expect("dialed address did not contain peerid in libp2p 0.34")
                            .with(c.info.remote_peer_id);
                Connection {
                    addr,
                    rtt: None
                }
            })
            .collect();

        Ok(cc)
    }

    /// Returns the local node public key and the listened and externally visible addresses.
    /// The addresses are suffixed with the P2p protocol containing the node's PeerId.
    ///
    /// Public key can be converted to [`PeerId`].
    pub async fn identity(&mut self) -> Result<(PublicKey, Vec<Multiaddr>), Error> {
        let ii = self.controls.swarm().retrieve_identify_info()
            .instrument(self.span.clone())
            .await?;
        Ok((ii.public_key, ii.listen_addrs))
    }

    /// Subscribes to a given topic. Can be done at most once without unsubscribing in the between.
    /// The subscription can be unsubscribed by dropping the stream or calling
    /// [`Ipfs::pubsub_unsubscribe`].
    pub async fn pubsub_subscribe(&mut self, topic: String) -> Result<Subscription, Error> {
        self.controls.pubsub().subscribe(Topic::new(topic))
            .instrument(self.span.clone())
            .await
            .map_err(Error::from)
    }

    /// Publishes to the topic which may have been subscribed to earlier
    pub async fn pubsub_publish(&mut self, topic: String, data: Vec<u8>) -> Result<(), Error> {
        self.controls.pubsub().publish(Topic::new(topic), data)
            .instrument(self.span.clone())
            .await
            .map_err(Error::from)
    }

    // /// Forcibly unsubscribes a previously made [`SubscriptionStream`], which could also be
    // /// unsubscribed by dropping the stream.
    // ///
    // /// Returns true if unsubscription was successful
    // pub async fn pubsub_unsubscribe(&self, topic: &str) -> Result<bool, Error> {
    //     async move {
    //         let (tx, rx) = oneshot_channel();
    //
    //         self.to_task
    //             .clone()
    //             .send(IpfsEvent::PubsubUnsubscribe(topic.into(), tx))
    //             .await?;
    //
    //         Ok(rx.await?)
    //     }
    //     .instrument(self.span.clone())
    //     .await
    // }
    //
    /// Returns all known pubsub peers with the optional topic filter
    pub async fn pubsub_peers(&mut self, topic: Option<String>) -> Result<Vec<PeerId>, Error> {
        let topic = if let Some(t) = topic {
            t
        } else {
            String::new()
        };

        self.controls.pubsub().get_peers(Topic::new(topic))
            .instrument(self.span.clone())
            .await
            .map_err(Error::from)
    }

    /// Returns all currently subscribed topics
    pub async fn pubsub_subscribed(&mut self) -> Result<Vec<String>, Error> {
        let topics = self.controls.pubsub().ls()
            .instrument(self.span.clone())
            .await
            .map_err(Error::from)?;

        let r = topics.into_iter().map(String::from).collect::<Vec<_>>();

        Ok(r)
    }

    /// Returns the known wantlist for the local node when the `peer` is `None` or the wantlist of the given `peer`
    pub async fn bitswap_wantlist(
        &mut self,
        peer: Option<PeerId>,
    ) -> Result<Vec<(Cid, bitswap::Priority)>, Error> {
        self.controls.bitswap().wantlist(peer)
            .instrument(self.span.clone())
            .await
            .map_err(Error::from)
    }

    /// Returns the statisctics of bitswap.
    pub async fn bitswap_stats(
        &mut self,
    ) -> Result<BitswapStats, Error> {
        let stats = self.controls.bitswap().stats()
            .instrument(self.span.clone())
            .await
            .map_err(Error::from)?;
        let peers = self.controls.bitswap().peers()
            .instrument(self.span.clone())
            .await
            .map_err(Error::from)?;
        let wantlist = self.controls.bitswap().wantlist(None)
            .instrument(self.span.clone())
            .await
            .map_err(Error::from)?;

        Ok(BitswapStats::from((stats, peers, wantlist)))
    }

    /// Obtain the addresses associated with the given `PeerId`; they are first searched for locally
    /// and the DHT is used as a fallback: a `Kademlia::get_closest_peers(peer_id)` query is run and
    /// when it's finished, the newly added DHT records are checked for the existence of the desired
    /// `peer_id` and if it's there, the list of its known addresses is returned.
    pub async fn find_peer(&mut self, peer_id: PeerId) -> Result<Vec<Multiaddr>, Error> {
        self.controls.kad().find_peer(&peer_id)
            .instrument(self.span.clone())
            .await
            .map(|kad_peer| kad_peer.multiaddrs)
            .map_err(Error::from)
    }

    /// Performs a DHT lookup for providers of a value to the given key.
    ///
    /// Returns a list of peers found providing the Cid.
    pub async fn get_providers(&mut self, cid: Cid) -> Result<Vec<PeerId>, Error> {
        self.controls.kad().find_providers(cid.to_bytes(), 1)
            .instrument(self.span.clone())
            .await
            .map(|peers| peers.into_iter().map(|p|p.node_id).collect())
            .map_err(Error::from)
    }

    /// Establishes the node as a provider of a block with the given Cid: it publishes a provider
    /// record with the given key (Cid) and the node's PeerId to the peers closest to the key. The
    /// publication of provider records is periodically repeated as per the interval specified in
    /// `libp2p`'s  `KademliaConfig`.
    pub async fn provide(&mut self, cid: Cid) -> Result<(), Error> {
        // don't provide things we don't actually have
        if self.repo.get_block_now(&cid).await?.is_none() {
            return Err(anyhow!(
                "Error: block {} not found locally, cannot provide",
                cid
            ));
        }

        self.controls.kad().provide(cid.to_bytes())
            .instrument(self.span.clone())
            .await
            .map_err(Error::from)
    }

    /// Returns a list of peers closest to the given `PeerId`, as suggested by the DHT. The
    /// node must have at least one known peer in its routing table in order for the query
    /// to return any values.
    pub async fn get_closest_peers(&mut self, peer_id: PeerId) -> Result<Vec<PeerId>, Error> {
        self.controls.kad().lookup(peer_id.to_bytes().into())
            .instrument(self.span.clone())
            .await
            .map(|peers| peers.into_iter().map(|p|p.node_id).collect())
            .map_err(Error::from)
    }

    /// Attempts to look a key up in the DHT and returns the values found in the records
    /// containing that key.
    // TODO: libp2p-rs only returns the first record...
    pub async fn dht_get<T: Into<Key>>(
        &mut self,
        key: T,
    ) -> Result<Vec<u8>, Error> {
        self.controls.kad().get_value(key.into().to_vec())
            .instrument(self.span.clone())
            .await
            .map_err(Error::from)
    }

    /// Stores the given key + value record locally and replicates it in the DHT. It doesn't
    /// expire locally and is periodically replicated in the DHT, as per the `KademliaConfig`
    /// setup.
    pub async fn dht_put<T: Into<Key>>(
        &mut self,
        key: T,
        value: Vec<u8>,
    ) -> Result<(), Error> {
        self.controls.kad().put_value(key.into().to_vec(), value)
            .instrument(self.span.clone())
            .await
            .map_err(Error::from)
    }

    /// Walk the given Iplds' links up to `max_depth` (or indefinitely for `None`). Will return
    /// any duplicate trees unless `unique` is `true`.
    ///
    /// More information and a `'static` lifetime version available at [`refs::iplds_refs`].
    pub fn refs<'a, Iter>(
        &'a self,
        iplds: Iter,
        max_depth: Option<u64>,
        unique: bool,
    ) -> impl Stream<Item = Result<refs::Edge, ipld::BlockError>> + Send + 'a
    where
        Iter: IntoIterator<Item = (Cid, Ipld)> + Send + 'a,
    {
        refs::iplds_refs(self, iplds, max_depth, unique)
    }

    /// Exit daemon.
    pub async fn exit_daemon(mut self) {
        self.repo.shutdown();

        self.controls.kad().close();
        self.controls.bitswap().close();
        self.controls.pubsub().close();

        self.controls.swarm().close();
        // TODO: close mdns...
    }


    pub async fn run_cli(mut self) {
        let mut app = App::new("xCLI");

        app.add_subcommand_with_userdata(ipfs_cli_commands(), Box::new(self.clone()));

        app.add_subcommand_with_userdata(swarm_cli_commands(), Box::new(self.controls.swarm().clone()));
        app.add_subcommand_with_userdata(dht_cli_commands(), Box::new(self.controls.kad().clone()));
        app.add_subcommand_with_userdata(bitswap_cli_commands(), Box::new(self.controls.bitswap().clone()));

        app.run();

        self.exit_daemon().await;
    }

}

/// Bitswap statistics
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BitswapStats {
    /// The number of IPFS blocks sent to other peers
    pub blocks_sent: u64,
    /// The number of bytes sent in IPFS blocks to other peers
    pub data_sent: u64,
    /// The number of IPFS blocks received from other peers
    pub blocks_received: u64,
    /// The number of bytes received in IPFS blocks from other peers
    pub data_received: u64,
    /// Duplicate blocks received (the block had already been received previously)
    pub dup_blks_received: u64,
    /// The number of bytes in duplicate blocks received
    pub dup_data_received: u64,
    /// The current peers
    pub peers: Vec<PeerId>,
    /// The wantlist of the local node
    pub wantlist: Vec<(Cid, bitswap::Priority)>,
}

impl
    From<(
        bitswap::Stats,
        Vec<PeerId>,
        Vec<(Cid, bitswap::Priority)>,
    )> for BitswapStats
{
    fn from(
        (stats, peers, wantlist): (
            bitswap::Stats,
            Vec<PeerId>,
            Vec<(Cid, bitswap::Priority)>,
        ),
    ) -> Self {
        BitswapStats {
            blocks_sent: stats.sent_blocks.load(Ordering::Relaxed),
            data_sent: stats.sent_data.load(Ordering::Relaxed),
            blocks_received: stats.received_blocks.load(Ordering::Relaxed),
            data_received: stats.received_data.load(Ordering::Relaxed),
            dup_blks_received: stats.duplicate_blocks.load(Ordering::Relaxed),
            dup_data_received: stats.duplicate_data.load(Ordering::Relaxed),
            peers,
            wantlist,
        }
    }
}

#[doc(hidden)]
pub use node::Node;
use crate::p2p::Controls;
use std::convert::TryFrom;
use libp2p_rs::floodsub::Topic;
use libp2p_rs::floodsub::subscription::Subscription;
use libp2p_rs::xcli::App;
use libp2p_rs::swarm::cli::swarm_cli_commands;
use libp2p_rs::kad::cli::dht_cli_commands;
use crate::cli::ipfs_cli_commands;
use crate::cli::bitswap_cli_commands;

/// Node module provides an easy to use interface used in `tests/`.
mod node {
    use super::*;
    use std::convert::TryFrom;

    /// Node encapsulates everything to setup a testing instance so that multi-node tests become
    /// easier.
    pub struct Node {
        /// The Ipfs facade.
        pub ipfs: Ipfs<TestTypes>,
        /// The peer identifier on the network.
        pub id: PeerId,
        /// The listened to and externally visible addresses. The addresses are suffixed with the
        /// P2p protocol containing the node's PeerID.
        pub addrs: Vec<Multiaddr>,
        /// Stores the single background task spawned for the node.
        pub bg_task: tokio::task::JoinHandle<()>,
    }

    impl Node {
        /// Initialises a new `Node` with an in-memory store backed configuration.
        ///
        /// This will use the testing defaults for the `IpfsOptions`. If `IpfsOptions` has been
        /// initialised manually, use `Node::with_options` instead.
        pub async fn new<T: AsRef<str>>(name: T) -> Self {
            let mut opts = IpfsOptions::inmemory_with_generated_keys();
            opts.span = Some(trace_span!("ipfs", node = name.as_ref()));
            Self::with_options(opts).await
        }

        /// Connects to a peer at the given address.
        pub async fn connect(&mut self, addr: Multiaddr) -> Result<(), Error> {
            let addr = MultiaddrWithPeerId::try_from(addr).unwrap();
            self.ipfs.connect(addr).await
        }

        /// Returns a new `Node` based on `IpfsOptions`.
        pub async fn with_options(opts: IpfsOptions) -> Self {
            let id = opts.keypair.public().into_peer_id();

            // for future: assume UninitializedIpfs handles instrumenting any futures with the
            // given span

            let (mut ipfs, fut): (Ipfs<TestTypes>, _) =
                UninitializedIpfs::new(opts).start().await.unwrap();
            let bg_task = tokio::task::spawn(fut);
            let addrs = ipfs.identity().await.unwrap().1;

            Node {
                ipfs,
                id,
                addrs,
                bg_task,
            }
        }

        /// Bootstraps the local node to join the DHT: it looks up the node's own ID in the
        /// DHT and introduces it to the other nodes in it; at least one other node must be
        /// known in order for the process to succeed. Subsequently, additional queries are
        /// ran with random keys so that the buckets farther from the closest neighbor also
        /// get refreshed.
        pub async fn bootstrap(&mut self) {
            self.controls.kad().bootstrap().await;
        }

        /// Add a known listen address of a peer participating in the DHT to the routing table.
        /// This is mandatory in order for the peer to be discoverable by other members of the
        /// DHT.
        pub async fn add_peer(&mut self, peer_id: PeerId, mut addr: Multiaddr) {
            // Kademlia::add_address requires the address to not contain the PeerId
            if matches!(addr.iter().last(), Some(Protocol::P2p(_))) {
                addr.pop();
            }

            self.controls.kad().add_node(peer_id, vec![addr]).await;
        }

        /// Returns the Bitswap peers for the a `Node`.
        pub async fn get_bitswap_peers(&self) -> Result<Vec<PeerId>, Error> {
            // let (tx, rx) = oneshot_channel();
            //
            // self.to_task
            //     .clone()
            //     .send(IpfsEvent::GetBitswapPeers(tx))
            //     .await?;
            //
            // rx.await.map_err(|e| anyhow!(e))
            Err(anyhow!("e"))
        }

        /// Shuts down the `Node`.
        pub async fn shutdown(self) {
            self.ipfs.exit_daemon().await;
            let _ = self.bg_task.await;
        }
    }

    impl Deref for Node {
        type Target = Ipfs<TestTypes>;

        fn deref(&self) -> &Self::Target {
            &self.ipfs
        }
    }

    impl DerefMut for Node {
        fn deref_mut(&mut self) -> &mut <Self as Deref>::Target {
            &mut self.ipfs
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::make_ipld;
    use multihash::Sha2_256;

    #[tokio::test]
    async fn test_put_and_get_block() {
        let ipfs = Node::new("test_node").await;

        let data = b"hello block\n".to_vec().into_boxed_slice();
        let cid = Cid::new_v1(Codec::Raw, Sha2_256::digest(&data));
        let block = Block::new(data, cid);

        let cid: Cid = ipfs.put_block(block.clone()).await.unwrap();
        let new_block = ipfs.get_block(&cid).await.unwrap();
        assert_eq!(block, new_block);
    }

    #[tokio::test]
    async fn test_put_and_get_dag() {
        let ipfs = Node::new("test_node").await;

        let data = make_ipld!([-1, -2, -3]);
        let cid = ipfs.put_dag(data.clone()).await.unwrap();
        let new_data = ipfs.get_dag(cid.into()).await.unwrap();
        assert_eq!(data, new_data);
    }

    #[tokio::test]
    async fn test_pin_and_unpin() {
        let ipfs = Node::new("test_node").await;

        let data = make_ipld!([-1, -2, -3]);
        let cid = ipfs.put_dag(data.clone()).await.unwrap();

        ipfs.insert_pin(&cid, false).await.unwrap();
        assert!(ipfs.is_pinned(&cid).await.unwrap());
        ipfs.remove_pin(&cid, false).await.unwrap();
        assert!(!ipfs.is_pinned(&cid).await.unwrap());
    }
}
