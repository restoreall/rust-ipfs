use libp2p::identity::Keypair;
use libp2p::multiaddr::Protocol;
use libp2p::{Multiaddr, PeerId};
use rand::{rngs::OsRng, Rng};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use thiserror::Error;

const BOOTSTRAP_NODES: &[&str] =
    &["/ip4/104.131.131.82/tcp/4001/p2p/QmaCpDMGvV2BGHeYERUEnRQAwe3N8SzbUtfsmvsqQLuvuJ"];

/// See test cases for examples how to write such file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConfigFile {
    #[serde(flatten)]
    key: KeyMaterial,
    bootstrap: Vec<Multiaddr>,
}

/// KeyMaterial is an additional abstraction as the identity::Keypair does not support Clone or
/// Debug nor is there a clearcut way to serialize and deserialize such yet at least.
#[derive(Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum KeyMaterial {
    Ed25519 {
        private_key: [u8; 32],
        #[serde(skip)]
        keypair: Option<Box<libp2p::identity::ed25519::Keypair>>,
    },
    RsaPkcs8File {
        #[serde(rename = "rsa_pkcs8_filename")]
        filename: String,
        #[serde(skip)]
        keypair: Option<Box<libp2p::identity::rsa::Keypair>>,
    },
}

use std::fmt;

impl fmt::Debug for KeyMaterial {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            KeyMaterial::Ed25519 { ref keypair, .. } => {
                if let Some(kp) = keypair.as_ref() {
                    write!(fmt, "{:?}", kp)
                } else {
                    write!(fmt, "Ed25519(not loaded)")
                }
            }
            KeyMaterial::RsaPkcs8File {
                ref keypair,
                ref filename,
            } => {
                if let Some(kp) = keypair.as_ref() {
                    write!(fmt, "{:?}", kp.public())
                } else {
                    write!(fmt, "Rsa(not loaded: {:?})", filename)
                }
            }
        }
    }
}

#[derive(Debug, Error)]
pub enum KeyMaterialLoadingFailure {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    RsaDecoding(#[from] libp2p::identity::error::DecodingError),
    #[error("{0}")]
    Config(#[from] serde_json::error::Error),
}

impl KeyMaterial {
    fn clone_keypair(&self) -> Keypair {
        match *self {
            KeyMaterial::Ed25519 { ref keypair, .. } => keypair
                .as_ref()
                .map(|kp| Keypair::Ed25519(kp.as_ref().clone())),
            KeyMaterial::RsaPkcs8File { ref keypair, .. } => {
                keypair.as_ref().map(|kp| Keypair::Rsa(kp.as_ref().clone()))
            }
        }
        .expect("KeyMaterial needs to be loaded before accessing the keypair")
    }

    fn load(&mut self) -> Result<(), KeyMaterialLoadingFailure> {
        match *self {
            KeyMaterial::Ed25519 {
                ref private_key,
                ref mut keypair,
            } if keypair.is_none() => {
                let mut cloned = *private_key;
                let sk = libp2p::identity::ed25519::SecretKey::from_bytes(&mut cloned)
                    .expect("Failed to extract ed25519::SecretKey");

                let kp = libp2p::identity::ed25519::Keypair::from(sk);

                *keypair = Some(Box::new(kp));
            }
            KeyMaterial::RsaPkcs8File {
                ref filename,
                ref mut keypair,
            } if keypair.is_none() => {
                let mut bytes = std::fs::read(filename).map_err(KeyMaterialLoadingFailure::Io)?;
                let kp = libp2p::identity::rsa::Keypair::from_pkcs8(&mut bytes)
                    .map_err(KeyMaterialLoadingFailure::RsaDecoding)?;
                *keypair = Some(Box::new(kp));
            }
            _ => { /* all set */ }
        }

        Ok(())
    }

    fn into_loaded(mut self) -> Result<Self, KeyMaterialLoadingFailure> {
        self.load()?;
        Ok(self)
    }
}

impl ConfigFile {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, KeyMaterialLoadingFailure> {
        if path.as_ref().exists() {
            let content = fs::read_to_string(&path)?;
            let config = Self::parse(&content)?;
            config.into_loaded()
        } else {
            let config = ConfigFile::default();
            config.store_at(path)?;
            config.into_loaded()
        }
    }

    fn load(&mut self) -> Result<(), KeyMaterialLoadingFailure> {
        self.key.load()
    }

    fn into_loaded(mut self) -> Result<Self, KeyMaterialLoadingFailure> {
        self.load()?;
        Ok(self)
    }

    fn parse(s: &str) -> Result<Self, KeyMaterialLoadingFailure> {
        Ok(serde_json::from_str(s)?)
    }

    pub fn store_at<P: AsRef<Path>>(&self, path: P) -> std::io::Result<()> {
        fs::create_dir_all(path.as_ref().parent().unwrap())?;
        let string = serde_json::to_string_pretty(self).unwrap();
        fs::write(path, string)
    }

    pub fn identity_key_pair(&self) -> Keypair {
        self.key.clone_keypair()
    }

    pub fn bootstrap(&self) -> Vec<(Multiaddr, PeerId)> {
        let mut bootstrap = Vec::new();
        for addr in &self.bootstrap {
            let mut addr = addr.to_owned();
            let peer_id = match addr.pop() {
                Some(Protocol::P2p(hash)) => PeerId::from_multihash(hash).unwrap(),
                _ => panic!("No peer id for addr"),
            };
            bootstrap.push((addr, peer_id));
        }
        bootstrap
    }
}

impl Default for ConfigFile {
    fn default() -> Self {
        // the ed25519 has no chance of working with go-ipfs as of now because of
        // https://github.com/libp2p/specs/issues/138 and
        // https://github.com/libp2p/go-libp2p-core/blob/dc718fa4dab1866476fd9f379718fdd619455a4f/peer/peer.go#L23-L34
        //
        // and on the other hand:
        // https://github.com/libp2p/rust-libp2p/blob/eb7b7bd919b93e6acf00847c19d1a76c09016120/core/src/peer_id.rs#L62-L74
        let private_key: [u8; 32] = OsRng.gen();

        let bootstrap = BOOTSTRAP_NODES
            .iter()
            .map(|node| node.parse().unwrap())
            .collect();
        ConfigFile {
            key: KeyMaterial::Ed25519 {
                private_key,
                keypair: None,
            }
            .into_loaded()
            .unwrap(),
            bootstrap,
        }
    }
}

#[cfg(test)]
mod tests {

    use super::ConfigFile;

    #[test]
    fn supports_older_v1_and_ed25519_v2() {
        let input = r#"{"private_key":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],"bootstrap":[]}"#;

        let actual = ConfigFile::parse(input).unwrap();

        let roundtrip = serde_json::to_string(&actual).unwrap();

        assert_eq!(input, roundtrip);
    }

    #[test]
    fn supports_v2() {
        let input = r#"{"rsa_pkcs8_filename":"foobar.pk8","bootstrap":[]}"#;

        let actual = ConfigFile::parse(input).unwrap();

        let roundtrip = serde_json::to_string(&actual).unwrap();

        assert_eq!(input, roundtrip);
    }
}
