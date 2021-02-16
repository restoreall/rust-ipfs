mod bitswap;
mod block;
mod control;
mod error;
mod ledger;
mod prefix;
mod protocol;
mod stat;

use cid::Cid;
use std::error::Error;
use std::fmt::Debug;

pub use bitswap::Bitswap;
pub use block::Block;
pub use control::Control;
pub use ledger::Priority;
pub use stat::Stats;

//pub use error::BitswapError;

const BS_PROTO_ID: &[u8] = b"/ipfs/bitswap/1.1.0";

mod bitswap_pb {
    include!(concat!(env!("OUT_DIR"), "/bitswap_pb.rs"));
}

/// This API is being discussed and evolved, which will likely lead to breakage.
// FIXME: why is this unpin? doesn't probably need to be since all of the futures are Box::pin'd.
pub trait BlockStore {
    /// Returns a block from the blockstore.
    fn get(&self, cid: &Cid) -> Result<Option<Block>, Box<dyn Error>>;
}

pub type IBlockStore = Box<dyn BlockStore + Send + Sync>;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
