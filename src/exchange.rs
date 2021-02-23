
use cid::Cid;

use async_trait::async_trait;
use bitswap::Block;

/// `Exchange` trait is used to describe the IPFS exchange protocol, f.g., Bitswap.
#[async_trait]
pub trait Exchange: Send {
    /// Retrieves the wanted block.
    async fn get_block(&mut self, cid: Cid) -> Result<Block, anyhow::Error>;

    /// Announces a new block.
    async fn has_block(&mut self, cid: Cid) -> Result<(), anyhow::Error>;

    fn box_clone(&self) -> IExchange;
}

pub type IExchange = Box<dyn Exchange>;

impl Clone for IExchange {
    fn clone(&self) -> Self {
        self.box_clone()
    }
}

#[async_trait]
impl Exchange for bitswap::Control {
    async fn get_block(&mut self, cid: Cid) -> Result<Block, anyhow::Error> {
        self.want_block(cid, 1).await.map_err(anyhow::Error::from)
    }

    async fn has_block(&mut self, cid: Cid) -> Result<(), anyhow::Error> {
        self.has_block(cid).await.map_err(anyhow::Error::from)
    }

    fn box_clone(&self) -> IExchange {
        Box::new(self.clone())
    }
}