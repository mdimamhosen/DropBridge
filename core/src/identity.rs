use anyhow::{Context, Result};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use sha2::{Digest, Sha256};

/// Long-term device identity: a self-signed ECDSA P-256 certificate and its key.
/// Apps persist `to_secret()` in OS secure storage (Keychain / Keystore-wrapped / Credential Manager).
#[derive(Clone)]
pub struct Identity {
    pub cert: Vec<u8>,
    pub key: Vec<u8>,
}

impl Identity {
    pub fn generate() -> Result<Self> {
        let ck = rcgen::generate_simple_self_signed(vec!["dropbridge".to_string()])?;
        Ok(Self { cert: ck.cert.der().to_vec(), key: ck.key_pair.serialize_der() })
    }

    pub fn to_secret(&self) -> String {
        format!("{}.{}", hex::encode(&self.cert), hex::encode(&self.key))
    }

    pub fn from_secret(s: &str) -> Result<Self> {
        let (c, k) = s.trim().split_once('.').context("stored identity is malformed")?;
        Ok(Self { cert: hex::decode(c)?, key: hex::decode(k)? })
    }

    pub fn fingerprint(&self) -> String {
        fingerprint(&self.cert)
    }

    pub(crate) fn cert_der(&self) -> CertificateDer<'static> {
        CertificateDer::from(self.cert.clone())
    }

    pub(crate) fn key_der(&self) -> PrivateKeyDer<'static> {
        PrivatePkcs8KeyDer::from(self.key.clone()).into()
    }
}

/// SHA-256 of the DER certificate, hex. This is the device id everywhere.
pub fn fingerprint(cert_der: &[u8]) -> String {
    hex::encode(Sha256::digest(cert_der))
}
