//! Identities and the encrypted transport (DESIGN §7.4), after SpacetimeDB:
//! an identity is a hash of a credential — here the peer's own ECDSA P-256
//! public key, so every node and CLI is its own issuer. Peers prove they
//! hold their key in a mutual TLS 1.3 handshake; nothing secret crosses
//! the wire.

use crate::{error::IntersticeError, node::NodeId};
use parking_lot::Mutex;
use rcgen::{CertificateParams, KeyPair, PublicKeyData};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::net::TcpStream;
use tokio_rustls::{
    TlsAcceptor, TlsConnector, TlsStream,
    rustls::{
        self, ClientConfig, DigitallySignedStruct, DistinguishedName, ServerConfig, SignatureScheme,
        client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
        crypto::CryptoProvider,
        pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime},
        server::danger::{ClientCertVerified, ClientCertVerifier},
    },
};

/// An encrypted connection to a peer.
pub type SecureStream = TlsStream<TcpStream>;

fn err(text: impl Into<String>) -> IntersticeError {
    IntersticeError::Internal(text.into())
}

/// The id a public key certifies: the first 16 bytes of SHA-256 over its
/// SubjectPublicKeyInfo, as a UUID v8.
pub fn derive_id(spki: &[u8]) -> NodeId {
    let hash = Sha256::digest(spki);
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&hash[..16]);
    uuid::Builder::from_custom_bytes(bytes).into_uuid()
}

fn fingerprint(spki: &[u8]) -> String {
    Sha256::digest(spki).iter().map(|b| format!("{b:02x}")).collect()
}

/// This node's (or CLI's) key pair and the TLS configs built from it.
pub struct LocalIdentity {
    pub id: NodeId,
    pub spki: Vec<u8>,
    client: Arc<ClientConfig>,
    server: Arc<ServerConfig>,
}

impl std::fmt::Debug for LocalIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalIdentity").field("id", &self.id).finish_non_exhaustive()
    }
}

impl LocalIdentity {
    /// A new key pair; its id is derived from it.
    pub fn generate() -> Result<(Self, String), IntersticeError> {
        let key = KeyPair::generate().map_err(|e| err(format!("key generation: {e}")))?;
        let pem = key.serialize_pem();
        let id = derive_id(&key.subject_public_key_info());
        Ok((Self::from_key(key, id)?, pem))
    }

    /// The key at `path`, or a new one written there. `id` is the identity
    /// to present: `None` for the id the key certifies (new nodes, CLIs),
    /// `Some` for a node created before keys (DESIGN §7.4).
    pub fn load_or_create(path: &Path, id: Option<NodeId>) -> Result<Self, IntersticeError> {
        let key = match std::fs::read_to_string(path) {
            Ok(pem) => KeyPair::from_pem(&pem).map_err(|e| err(format!("{}: {e}", path.display())))?,
            Err(_) => {
                let key = KeyPair::generate().map_err(|e| err(format!("key generation: {e}")))?;
                write_private(path, &key.serialize_pem())?;
                key
            }
        };
        let id = id.unwrap_or_else(|| derive_id(&key.subject_public_key_info()));
        Self::from_key(key, id)
    }

    fn from_key(key: KeyPair, id: NodeId) -> Result<Self, IntersticeError> {
        let spki = key.subject_public_key_info();
        let cert = CertificateParams::new(vec!["interstice".to_string()])
            .and_then(|p| p.self_signed(&key))
            .map_err(|e| err(format!("certificate: {e}")))?;
        let cert = cert.der().clone();
        let private = || PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der()));
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        let verifier = Arc::new(AnyPeer(provider.clone()));
        let client = ClientConfig::builder_with_provider(provider.clone())
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|e| err(e.to_string()))?
            .dangerous()
            .with_custom_certificate_verifier(verifier.clone())
            .with_client_auth_cert(vec![cert.clone()], private())
            .map_err(|e| err(format!("client TLS config: {e}")))?;
        let server = ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|e| err(e.to_string()))?
            .with_client_cert_verifier(verifier)
            .with_single_cert(vec![cert], private())
            .map_err(|e| err(format!("server TLS config: {e}")))?;
        Ok(Self { id, spki, client: Arc::new(client), server: Arc::new(server) })
    }

    /// Dial `address`; returns the stream and the peer's public key.
    pub async fn connect(&self, address: &str) -> Result<(SecureStream, Vec<u8>), IntersticeError> {
        let tcp = TcpStream::connect(address)
            .await
            .map_err(|e| err(format!("Failed to connect to node at {address}: {e}")))?;
        let _ = tcp.set_nodelay(true);
        let name = ServerName::try_from("interstice").expect("static name");
        let stream = TlsConnector::from(self.client.clone())
            .connect(name, tcp)
            .await
            .map_err(|e| err(format!("TLS handshake with {address} failed: {e}")))?;
        let spki = peer_spki(stream.get_ref().1.peer_certificates())?;
        Ok((TlsStream::from(stream), spki))
    }

    /// Accept a dialed connection; returns the stream and the peer's key.
    pub async fn accept(&self, tcp: TcpStream) -> Result<(SecureStream, Vec<u8>), IntersticeError> {
        let _ = tcp.set_nodelay(true);
        let stream = TlsAcceptor::from(self.server.clone())
            .accept(tcp)
            .await
            .map_err(|e| err(format!("TLS handshake failed: {e}")))?;
        let spki = peer_spki(stream.get_ref().1.peer_certificates())?;
        Ok((TlsStream::from(stream), spki))
    }
}

fn write_private(path: &Path, text: &str) -> Result<(), IntersticeError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| err(e.to_string()))?;
    }
    std::fs::write(path, text).map_err(|e| err(format!("{}: {e}", path.display())))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// Save a key generated with [`LocalIdentity::generate`].
pub fn save_key(path: &Path, pem: &str) -> Result<(), IntersticeError> {
    write_private(path, pem)
}

fn peer_spki(certs: Option<&[CertificateDer<'static>]>) -> Result<Vec<u8>, IntersticeError> {
    let cert = certs.and_then(|c| c.first()).ok_or_else(|| err("peer presented no certificate"))?;
    let parsed = webpki::EndEntityCert::try_from(cert).map_err(|e| err(format!("peer certificate: {e}")))?;
    Ok(parsed.subject_public_key_info().as_ref().to_vec())
}

/// Accepts any certificate but checks every handshake signature, so the peer
/// proves it holds the key in its certificate. Trust is the identity, not a
/// CA chain (as in libp2p's TLS transport).
#[derive(Debug)]
struct AnyPeer(Arc<CryptoProvider>);

impl ServerCertVerifier for AnyPeer {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::General("TLS 1.2 is not supported".into()))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

impl ClientCertVerifier for AnyPeer {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, rustls::Error> {
        Ok(ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::General("TLS 1.2 is not supported".into()))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// Keys remembered for ids that don't certify themselves (nodes created
/// before keys), like SSH's `known_hosts` (DESIGN §7.4).
pub struct KnownPeers {
    path: Option<PathBuf>,
    keys: Mutex<BTreeMap<String, String>>,
}

impl KnownPeers {
    pub fn load(path: Option<PathBuf>) -> Self {
        let keys = path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| toml::from_str(&t).ok())
            .unwrap_or_default();
        Self { path, keys: Mutex::new(keys) }
    }

    /// Check that `spki` may present `claimed`: it certifies that id, or it
    /// is the key remembered for it (remembered now if first seen).
    pub fn bind(&self, claimed: NodeId, spki: &[u8]) -> Result<(), IntersticeError> {
        if derive_id(spki) == claimed {
            return Ok(());
        }
        // A key-derived id (UUID v8) is only ever presented by its key;
        // only legacy ids (v4) are remembered on first use.
        if claimed.get_version_num() == 8 {
            return Err(IntersticeError::PermissionDenied(format!(
                "{claimed} is not the identity of the key presented"
            )));
        }
        let print = fingerprint(spki);
        let mut keys = self.keys.lock();
        match keys.get(&claimed.to_string()) {
            Some(known) if *known == print => Ok(()),
            Some(_) => Err(IntersticeError::PermissionDenied(format!(
                "{claimed} presented a different key than before"
            ))),
            None => {
                keys.insert(claimed.to_string(), print);
                if let Some(path) = &self.path {
                    if let Ok(text) = toml::to_string_pretty(&*keys) {
                        let _ = std::fs::write(path, text);
                    }
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mutual_tls_binds_identities() {
        let (a, _) = LocalIdentity::generate().unwrap();
        let (b, _) = LocalIdentity::generate().unwrap();
        assert_eq!(a.id, derive_id(&a.spki));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let b = Arc::new(b);
        let server = {
            let b = b.clone();
            tokio::spawn(async move {
                let (tcp, _) = listener.accept().await.unwrap();
                let (mut stream, spki) = b.accept(tcp).await.unwrap();
                crate::network::packet::write_packet(&mut stream, &crate::network::protocol::NetworkPacket::Close)
                    .await
                    .unwrap();
                spki
            })
        };
        let (mut stream, server_spki) = a.connect(&addr).await.unwrap();
        let packet = crate::network::packet::read_packet(&mut stream).await.unwrap();
        assert!(matches!(packet, crate::network::protocol::NetworkPacket::Close));
        let client_spki = server.await.unwrap();
        // Each side learns the other's real key.
        assert_eq!(derive_id(&server_spki), b.id);
        assert_eq!(derive_id(&client_spki), a.id);

        // Binding: self-certified ids pass; a legacy id is pinned on first use.
        let known = KnownPeers::load(None);
        known.bind(b.id, &server_spki).unwrap();
        assert!(known.bind(a.id, &server_spki).is_err());
        let legacy = uuid::Uuid::new_v4();
        known.bind(legacy, &server_spki).unwrap();
        known.bind(legacy, &server_spki).unwrap();
        assert!(known.bind(legacy, &client_spki).is_err());
    }
}

/// What the network authenticates with: this node's identity and the keys
/// it remembers for legacy peers.
pub struct Trust {
    pub identity: LocalIdentity,
    pub known: KnownPeers,
}
