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

/// One step of a key rotation (DESIGN §7.4): the key before it signs
/// `next_key` as the identity's next key.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct KeyHandover {
    pub next_key: Vec<u8>,
    pub signature: Vec<u8>,
}

/// How an identity's current key descends from its first key, from which
/// the id is made. Never rotated: no handovers.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct KeyChain {
    pub first_key: Vec<u8>,
    pub handovers: Vec<KeyHandover>,
}

/// What handover `seq` of `id` signs.
fn handover_message(id: NodeId, seq: usize, next_key: &[u8]) -> Vec<u8> {
    let mut msg = b"interstice key handover\0".to_vec();
    msg.extend_from_slice(id.as_bytes());
    msg.extend_from_slice(&(seq as u64).to_le_bytes());
    msg.extend_from_slice(next_key);
    msg
}

/// The SubjectPublicKeyInfo header of an ECDSA P-256 key, before its point.
const P256_SPKI_HEADER: [u8; 26] = [
    0x30, 0x59, 0x30, 0x13, 0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06, 0x08, 0x2a, 0x86, 0x48,
    0xce, 0x3d, 0x03, 0x01, 0x07, 0x03, 0x42, 0x00,
];

fn verify_p256(spki: &[u8], msg: &[u8], signature: &[u8]) -> bool {
    use aws_lc_rs::signature::{ECDSA_P256_SHA256_ASN1, UnparsedPublicKey};
    spki.len() == P256_SPKI_HEADER.len() + 65
        && spki.starts_with(&P256_SPKI_HEADER)
        && UnparsedPublicKey::new(&ECDSA_P256_SHA256_ASN1, &spki[P256_SPKI_HEADER.len()..])
            .verify(msg, signature)
            .is_ok()
}

impl KeyChain {
    /// The id this chain belongs to.
    pub fn id(&self) -> NodeId {
        derive_id(&self.first_key)
    }

    /// The key it ends at.
    pub fn current_key(&self) -> &[u8] {
        self.handovers.last().map(|h| h.next_key.as_slice()).unwrap_or(&self.first_key)
    }

    /// Check that the chain is `claimed`'s and ends at `spki`; returns how
    /// many handovers it has.
    pub fn verify(&self, claimed: NodeId, spki: &[u8]) -> Result<usize, IntersticeError> {
        let denied = |why: String| IntersticeError::PermissionDenied(why);
        if self.id() != claimed {
            return Err(denied(format!("{claimed} is not the identity of the key presented")));
        }
        let mut key = self.first_key.as_slice();
        for (i, handover) in self.handovers.iter().enumerate() {
            if !verify_p256(key, &handover_message(claimed, i + 1, &handover.next_key), &handover.signature) {
                return Err(denied(format!("{claimed}: key handover {} is not signed by the key before it", i + 1)));
            }
            key = &handover.next_key;
        }
        if key != spki {
            return Err(denied(format!("{claimed}: the key presented is not the identity's current key")));
        }
        Ok(self.handovers.len())
    }
}

/// This node's (or CLI's) key pair, its rotation chain, and the TLS configs
/// built from the key.
pub struct LocalIdentity {
    pub id: NodeId,
    pub spki: Vec<u8>,
    pub chain: KeyChain,
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
        let chain = KeyChain { first_key: key.subject_public_key_info(), handovers: Vec::new() };
        Ok((Self::from_key(key, chain)?, pem))
    }

    /// The key at `path` and its chain (`<path>.chain`, DESIGN §7.4), or a
    /// new key written there.
    pub fn load_or_create(path: &Path) -> Result<Self, IntersticeError> {
        let key = match std::fs::read_to_string(path) {
            Ok(pem) => KeyPair::from_pem(&pem).map_err(|e| err(format!("{}: {e}", path.display())))?,
            Err(_) => {
                let key = KeyPair::generate().map_err(|e| err(format!("key generation: {e}")))?;
                write_private(path, &key.serialize_pem())?;
                key
            }
        };
        let chain = read_chain(path)?;
        let spki = key.subject_public_key_info();
        if chain.handovers.is_empty() {
            return Self::from_key(key, KeyChain { first_key: spki, handovers: Vec::new() });
        }
        if chain.current_key() == spki {
            return Self::from_key(key, chain);
        }
        // A rotation interrupted after the chain was written: finish it.
        let pending = pending_path(path);
        let pem = std::fs::read_to_string(&pending)
            .map_err(|_| err(format!("{}: the key is not the chain's current key", path.display())))?;
        let next = KeyPair::from_pem(&pem).map_err(|e| err(format!("{}: {e}", pending.display())))?;
        if next.subject_public_key_info() != chain.current_key() {
            return Err(err(format!("{}: the key is not the chain's current key", path.display())));
        }
        std::fs::rename(&pending, path).map_err(|e| err(format!("{}: {e}", path.display())))?;
        Self::from_key(next, chain)
    }

    /// Move the identity whose key is at `path` to a new key, keeping its id
    /// (DESIGN §7.4): the current key signs the handover, the chain is
    /// written, then the key file is replaced. Returns the new identity.
    pub fn rotate(path: &Path) -> Result<Self, IntersticeError> {
        let current = Self::load_or_create(path)?;
        let pem = std::fs::read_to_string(path).map_err(|e| err(format!("{}: {e}", path.display())))?;
        let key = KeyPair::from_pem(&pem).map_err(|e| err(format!("{}: {e}", path.display())))?;
        let next = KeyPair::generate().map_err(|e| err(format!("key generation: {e}")))?;
        let next_key = next.subject_public_key_info();
        let seq = current.chain.handovers.len() + 1;
        let signature = rcgen::SigningKey::sign(&key, &handover_message(current.id, seq, &next_key))
            .map_err(|e| err(format!("signing the handover: {e}")))?;
        let mut chain = current.chain.clone();
        chain.handovers.push(KeyHandover { next_key, signature });
        chain.verify(current.id, chain.current_key())?;
        // Key first (aside), then the chain, then the swap: a crash in
        // between is finished by `load_or_create`.
        write_private(&pending_path(path), &next.serialize_pem())?;
        let encoded = interstice_abi::encode(&chain).map_err(|e| err(e.to_string()))?;
        let chain_tmp = chain_path(path).with_extension("chain.tmp");
        std::fs::write(&chain_tmp, encoded).map_err(|e| err(e.to_string()))?;
        std::fs::rename(&chain_tmp, chain_path(path)).map_err(|e| err(e.to_string()))?;
        std::fs::rename(pending_path(path), path).map_err(|e| err(e.to_string()))?;
        Self::from_key(next, chain)
    }

    fn from_key(key: KeyPair, chain: KeyChain) -> Result<Self, IntersticeError> {
        let spki = key.subject_public_key_info();
        let id = chain.id();
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
        Ok(Self { id, spki, chain, client: Arc::new(client), server: Arc::new(server) })
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

/// `identity.key` → `identity.chain`.
fn chain_path(key: &Path) -> PathBuf {
    key.with_extension("chain")
}

/// The next key while a rotation is under way.
fn pending_path(key: &Path) -> PathBuf {
    key.with_extension("key.next")
}

fn read_chain(key: &Path) -> Result<KeyChain, IntersticeError> {
    match std::fs::read(chain_path(key)) {
        Ok(bytes) => interstice_abi::decode(&bytes).map_err(|e| err(format!("{}: {e}", chain_path(key).display()))),
        Err(_) => Ok(KeyChain::default()),
    }
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

/// The newest key handover seen for each identity that has rotated
/// (DESIGN §7.4): id → `seq` and the fingerprint of the key it ends at.
pub struct KnownPeers {
    path: Option<PathBuf>,
    keys: Mutex<BTreeMap<String, (u64, String)>>,
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

    /// Check that `spki`, with `chain`, may present `claimed`: the chain is
    /// valid and not older than, nor a fork of, the newest one seen.
    pub fn bind(&self, claimed: NodeId, chain: &KeyChain, spki: &[u8]) -> Result<(), IntersticeError> {
        let seq = chain.verify(claimed, spki)? as u64;
        let print = fingerprint(spki);
        let mut keys = self.keys.lock();
        match keys.get(&claimed.to_string()) {
            Some((known, _)) if seq < *known => Err(IntersticeError::PermissionDenied(format!(
                "{claimed} presented a key it has rotated away from"
            ))),
            Some((known, known_print)) if seq == *known && *known_print != print => {
                Err(IntersticeError::PermissionDenied(format!(
                    "{claimed} presented a key handover that conflicts with one seen before"
                )))
            }
            Some((known, _)) if seq == *known => Ok(()),
            _ if seq == 0 => Ok(()),
            _ => {
                keys.insert(claimed.to_string(), (seq, print));
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

        // Binding: an id passes only with its own key.
        let known = KnownPeers::load(None);
        known.bind(b.id, &b.chain, &server_spki).unwrap();
        assert!(known.bind(a.id, &b.chain, &server_spki).is_err());
        assert!(known.bind(b.id, &b.chain, &client_spki).is_err());
    }

    #[test]
    fn rotation_keeps_the_id_and_retires_the_old_key() {
        let dir = std::env::temp_dir().join(format!("interstice-rotate-{}", uuid::Uuid::new_v4()));
        let path = dir.join("identity.key");
        let first = LocalIdentity::load_or_create(&path).unwrap();
        let second = LocalIdentity::rotate(&path).unwrap();
        let third = LocalIdentity::rotate(&path).unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(first.id, third.id);
        assert_ne!(first.spki, third.spki);
        // Reloading gives the rotated key and its chain.
        let again = LocalIdentity::load_or_create(&path).unwrap();
        assert_eq!(again.spki, third.spki);
        assert_eq!(again.chain.handovers.len(), 2);

        let known = KnownPeers::load(None);
        known.bind(first.id, &first.chain, &first.spki).unwrap();
        known.bind(second.id, &second.chain, &second.spki).unwrap();
        // Once seq 1 is seen, the first key is refused, even with its
        // (valid) chain.
        assert!(known.bind(first.id, &first.chain, &first.spki).is_err());
        known.bind(third.id, &third.chain, &third.spki).unwrap();
        assert!(known.bind(second.id, &second.chain, &second.spki).is_err());

        // A fork: the first key signs another handover at seq 1.
        let (other, _) = LocalIdentity::generate().unwrap();
        let mut forged = second.chain.clone();
        forged.handovers[0].next_key = other.spki.clone();
        assert!(forged.verify(first.id, &other.spki).is_err(), "the signature covers the next key");

        // A tampered chain or a key that is not the chain's end.
        let mut bad = third.chain.clone();
        bad.handovers[1].signature[10] ^= 1;
        assert!(bad.verify(first.id, &third.spki).is_err());
        assert!(third.chain.verify(first.id, &second.spki).is_err());

        // An interrupted rotation (chain written, key not swapped yet)
        // finishes on load.
        let third_pem = std::fs::read_to_string(&path).unwrap();
        let fourth = LocalIdentity::rotate(&path).unwrap();
        let pending = super::pending_path(&path);
        std::fs::rename(&path, &pending).unwrap();
        std::fs::write(&path, third_pem).unwrap();
        let resumed = LocalIdentity::load_or_create(&path).unwrap();
        assert_eq!(resumed.spki, fourth.spki);
        assert!(!pending.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// What the network authenticates with: this node's identity and the
/// newest key handovers it has seen.
pub struct Trust {
    pub identity: LocalIdentity,
    pub known: KnownPeers,
}
