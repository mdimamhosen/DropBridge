//! Mutual TLS 1.3 with self-signed certs. The handshake accepts any certificate
//! (signatures are still verified); trust is decided by fingerprint at the app layer.
use crate::identity::{fingerprint, Identity};
use anyhow::{Context, Result};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider, WebPkiSupportedAlgorithms};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{ClientConfig, DigitallySignedStruct, DistinguishedName, ServerConfig, SignatureScheme};
use sha2::{Digest, Sha256};
use std::sync::Arc;

#[derive(Debug)]
struct AnyCert(WebPkiSupportedAlgorithms);

impl ServerCertVerifier for AnyCert {
    fn verify_server_cert(
        &self,
        _: &CertificateDer,
        _: &[CertificateDer],
        _: &ServerName,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        m: &[u8],
        c: &CertificateDer,
        d: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(m, c, d, &self.0)
    }
    fn verify_tls13_signature(
        &self,
        m: &[u8],
        c: &CertificateDer,
        d: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(m, c, d, &self.0)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.supported_schemes()
    }
}

impl ClientCertVerifier for AnyCert {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }
    fn verify_client_cert(&self, _: &CertificateDer, _: &[CertificateDer], _: UnixTime) -> Result<ClientCertVerified, rustls::Error> {
        Ok(ClientCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        m: &[u8],
        c: &CertificateDer,
        d: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(m, c, d, &self.0)
    }
    fn verify_tls13_signature(
        &self,
        m: &[u8],
        c: &CertificateDer,
        d: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(m, c, d, &self.0)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.supported_schemes()
    }
}

fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

pub fn server_config(id: &Identity) -> Result<Arc<ServerConfig>> {
    let p = provider();
    let v = Arc::new(AnyCert(p.signature_verification_algorithms));
    let cfg = ServerConfig::builder_with_provider(p)
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_client_cert_verifier(v)
        .with_single_cert(vec![id.cert_der()], id.key_der())?;
    Ok(Arc::new(cfg))
}

pub fn client_config(id: &Identity) -> Result<Arc<ClientConfig>> {
    let p = provider();
    let v = Arc::new(AnyCert(p.signature_verification_algorithms));
    let cfg = ClientConfig::builder_with_provider(p)
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .dangerous()
        .with_custom_certificate_verifier(v)
        .with_client_auth_cert(vec![id.cert_der()], id.key_der())?;
    Ok(Arc::new(cfg))
}

pub struct PeerTls {
    pub fingerprint: String,
    /// 6-digit short authentication string; equal on both ends only without a MITM.
    pub sas: String,
}

pub fn peer_of<D>(c: &rustls::ConnectionCommon<D>) -> Result<PeerTls> {
    let cert = c.peer_certificates().and_then(|v| v.first()).context("peer sent no certificate")?;
    let ekm = c.export_keying_material([0u8; 32], b"EXPORTER-dropbridge-sas", None)?;
    let d = Sha256::digest(ekm);
    let n = u32::from_be_bytes([d[0], d[1], d[2], d[3]]) % 1_000_000;
    Ok(PeerTls { fingerprint: fingerprint(cert), sas: format!("{n:06}") })
}

pub fn peer(s: &tokio_rustls::TlsStream<tokio::net::TcpStream>) -> Result<PeerTls> {
    match s {
        tokio_rustls::TlsStream::Client(c) => peer_of(c.get_ref().1),
        tokio_rustls::TlsStream::Server(c) => peer_of(c.get_ref().1),
    }
}
