//! go-ipfs compatible configuration file handling and setup.

use ipfs::{multiaddr, secp256k1, Keypair, Multiaddr};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::path::Path;
use std::str::FromStr;
use structopt::StructOpt;
use thiserror::Error;

/// Temporary module required to de/ser config files base64'd protobuf rsa private key format.
/// Temporary until the private key import/export can be PR'd into rust-libp2p.
mod keys_proto {
    include!(concat!(env!("OUT_DIR"), "/keys_proto.rs"));
}

/// Defines the configuration types supported by the API.
#[derive(Debug, StructOpt)]
pub enum Profile {
    Test,
    Default,
}

// Required for structopt.
impl FromStr for Profile {
    type Err = InitializationError;

    fn from_str(profile: &str) -> Result<Self, Self::Err> {
        match profile {
            "test" => Ok(Profile::Test),
            "default" => Ok(Profile::Default),
            _ => Err(InitializationError::InvalidProfile(profile.to_string())),
        }
    }
}

/// The way things can go wrong when calling [`init`].
#[derive(Error, Debug)]
pub enum InitializationError {
    #[error("repository creation failed: {0}")]
    DirectoryCreationFailed(std::io::Error),
    #[error("configuration file creation failed: {0}")]
    ConfigCreationFailed(std::io::Error),
    #[error("invalid RSA key length given: {0}")]
    InvalidRsaKeyLength(u16),
    #[error("unsupported profiles selected: {0:?}")]
    InvalidProfile(String),
    #[error("key generation failed: {0}")]
    KeyGeneration(Box<dyn std::error::Error + 'static>),
    #[error("key encoding failed: {0}")]
    PrivateKeyEncodingFailed(prost::EncodeError),
    #[error("config serialization failed: {0}")]
    ConfigWritingFailed(Box<dyn std::error::Error + 'static>),
}

/// Creates the IPFS_PATH directory structure and creates a new compatible configuration file with
/// Secp256k1 key. Returns the Peer ID.
pub fn init(ipfs_path: &Path, profiles: Vec<Profile>) -> Result<String, InitializationError> {
    use multibase::Base::Base64Pad;
    use prost::Message;
    use std::fs::OpenOptions;
    use std::io::{BufWriter, Write};

    if profiles.len() != 1 {
        unimplemented!("Multiple profiles are currently unsupported!")
    }

    let inner_kpair = secp256k1::Keypair::generate();
    let kp = inner_kpair.clone();
    let private = kp.secret();

    let keypair = Keypair::Secp256k1(inner_kpair);

    let peer_id = keypair.public().into_peer_id().to_string();

    // TODO: this part could be PR'd to rust-libp2p as they already have some public key
    // import/export but probably not if ring does not support these required conversions.

    let key_desc = keys_proto::PrivateKey {
        r#type: keys_proto::KeyType::Secp256k1 as i32,
        data: private.to_bytes().to_vec(),
    };

    let private_key = {
        let mut buf = Vec::with_capacity(key_desc.encoded_len());
        key_desc
            .encode(&mut buf)
            .map_err(InitializationError::PrivateKeyEncodingFailed)?;
        buf
    };

    let private_key = Base64Pad.encode(&private_key);

    let api_addr = match profiles[0] {
        Profile::Test => multiaddr!(Ip4([127, 0, 0, 1]), Tcp(0u16)),
        Profile::Default => multiaddr!(Ip4([127, 0, 0, 1]), Tcp(4004u16)),
    };

    let config_contents = CompatibleConfigFile {
        identity: Identity {
            peer_id: peer_id.clone(),
            private_key,
        },
        addresses: Addresses {
            swarm: vec!["/ip4/127.0.0.1/tcp/0".parse().unwrap()],
            api: api_addr,
        },
    };

    let config_path = ipfs_path.join("config");

    let config_file = fs::create_dir_all(&ipfs_path)
        .map_err(InitializationError::DirectoryCreationFailed)
        .and_then(|_| {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&config_path)
                .map_err(InitializationError::ConfigCreationFailed)
        })?;

    let mut writer = BufWriter::new(config_file);

    serde_json::to_writer_pretty(&mut writer, &config_contents)
        .map_err(|e| InitializationError::ConfigWritingFailed(Box::new(e)))?;

    writer
        .flush()
        .map_err(|e| InitializationError::ConfigWritingFailed(Box::new(e)))?;

    Ok(peer_id)
}

/// The facade for the configuration of the API.
pub struct Config {
    /// Keypair for the ipfs node.
    pub keypair: ipfs::Keypair,
    /// Peer addresses for the ipfs node.
    pub swarm: Vec<Multiaddr>,
    /// Address to run the API daemon on.
    pub api_addr: Multiaddr,
}

/// Things which can go wrong when loading a `go-ipfs` compatible configuration file.
#[derive(Error, Debug)]
pub enum LoadingError {
    #[error("failed to open the configuration file: {0}")]
    ConfigurationFileOpening(std::io::Error),
    #[error("failed to read the configuration file: {0}")]
    ConfigurationFileFormat(serde_json::Error),
    #[error("failed to load the private key: {0}")]
    PrivateKeyLoadingFailed(Box<dyn std::error::Error + 'static>),
    #[error("unsupported private key format: {0}")]
    UnsupportedPrivateKeyType(i32),
    #[error("loaded PeerId {loaded:?} is not the same as in configuration file {stored:?}, this is likely a bug in rust-ipfs-http")]
    PeerIdMismatch { loaded: String, stored: String },
}

/// Loads a `go-ipfs` compatible configuration file from the given file.
///
/// Returns only the keypair and listening addresses or [`LoadingError`] but this should be
/// extended to contain the bootstrap nodes at least later when we need to support those for
/// testing purposes.
pub fn load(config: File) -> Result<Config, LoadingError> {
    use std::io::BufReader;

    let config_file: CompatibleConfigFile = serde_json::from_reader(BufReader::new(config))
        .map_err(LoadingError::ConfigurationFileFormat)?;

    let kp = config_file.identity.load_keypair()?;

    let peer_id = kp.public().into_peer_id().to_string();

    if peer_id != config_file.identity.peer_id {
        return Err(LoadingError::PeerIdMismatch {
            loaded: peer_id,
            stored: config_file.identity.peer_id,
        });
    }

    let config = Config {
        keypair: kp,
        swarm: config_file.addresses.swarm,
        api_addr: config_file.addresses.api,
    };

    Ok(config)
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct CompatibleConfigFile {
    identity: Identity,
    addresses: Addresses,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Addresses {
    swarm: Vec<Multiaddr>,
    #[serde(rename = "API")]
    api: Multiaddr,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Identity {
    #[serde(rename = "PeerID")]
    peer_id: String,
    #[serde(rename = "PrivKey")]
    private_key: String,
}

impl Identity {
    fn load_keypair(&self) -> Result<ipfs::Keypair, LoadingError> {
        use keys_proto::KeyType;
        use multibase::Base::Base64Pad;
        use prost::Message;

        let bytes = Base64Pad
            .decode(&self.private_key)
            .map_err(|e| LoadingError::PrivateKeyLoadingFailed(Box::new(e)))?;

        let private_key = keys_proto::PrivateKey::decode(bytes.as_slice())
            .map_err(|e| LoadingError::PrivateKeyLoadingFailed(Box::new(e)))?;

        Ok(match KeyType::from_i32(private_key.r#type) {
            Some(KeyType::Secp256k1) => {
                let pk = secp256k1::SecretKey::from_bytes(private_key.data.to_vec())
                    .map_err(|e| LoadingError::PrivateKeyLoadingFailed(Box::new(e)))?;

                Keypair::Secp256k1(secp256k1::Keypair::from(pk))
            }
            _keytype => return Err(LoadingError::UnsupportedPrivateKeyType(private_key.r#type)),
        })
    }
}

#[cfg(test)]
mod test {

    #[test]
    fn read_private_key_from_goipfs() {
        use super::Identity;

        // generated with go-ipfs 0.4.23, init --bits 2048
        let input = Identity {
            peer_id: String::from("16Uiu2HAmE9yoxJAZTYjYAGAVdGTPJsLYKqzDH8QeRLWpjM6fusFS"),
            private_key: String::from("CAISIPdo1BBbNHESusJmhbWsvser0cvUqXeuYEn0dAr7lUt1"),
        };

        let peer_id = input
            .load_keypair()
            .unwrap()
            .public()
            .into_peer_id()
            .to_string();

        assert_eq!(peer_id, input.peer_id);
    }
}
