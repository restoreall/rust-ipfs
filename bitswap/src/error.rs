use thiserror::Error;

#[derive(Debug, Error)]
pub enum BitswapError {
    #[error("Error while decoding bitswap message: {0}")]
    ProtobufError(#[from] prost::DecodeError),
    #[error("Error while parsing cid: {0}")]
    Cid(#[from] cid::Error),
}
