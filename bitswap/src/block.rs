use cid::Cid;
use std::fmt::Debug;
use std::error::Error;

use async_trait::async_trait;

/// An Ipfs block consisting of a [`Cid`] and the bytes of the block.
///
/// Note: At the moment the equality is based on [`Cid`] equality, which is based on the triple
/// `(cid::Version, cid::Codec, multihash)`.
#[derive(Clone, Debug)]
pub struct Block {
    /// The content identifier for this block
    pub cid: Cid,
    /// The data of this block
    pub data: Box<[u8]>,
}

impl PartialEq for Block {
    fn eq(&self, other: &Self) -> bool {
        self.cid.hash() == other.cid.hash()
    }
}

impl Eq for Block {}

impl Block {
    pub fn new(data: Box<[u8]>, cid: Cid) -> Self {
        Self { cid, data }
    }

    pub fn cid(&self) -> &Cid {
        &self.cid
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn into_vec(self) -> Vec<u8> {
        self.data.into()
    }
}

/// BlockStore TRait used by Bitswap.
#[async_trait]
pub trait BsBlockStore: Clone + Send + Sync + Unpin + 'static {
    /// Returns whether a block is present in the blockstore.
    async fn contains(&self, cid: &Cid) -> Result<bool, Box<dyn Error>>;
    /// Returns a block from the blockstore.
    async fn get(&self, cid: &Cid) -> Result<Option<Block>, Box<dyn Error>>;
    /// Inserts a block in the blockstore.
    async fn put(&self, block: Block) -> Result<Cid, Box<dyn Error>>;
    /// Removes a block from the blockstore.
    async fn remove(&self, cid: &Cid) -> Result<(), Box<dyn Error>>;
    // /// Returns a list of the blocks (Cids), in the blockstore.
    // async fn list(&self) -> Result<Vec<Cid>, Error>;
}
