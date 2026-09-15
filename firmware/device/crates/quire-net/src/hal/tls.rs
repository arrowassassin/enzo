//! TLS 1.3 for the fetchers: embedded-tls with Quire's own certificate verifier
//! (`proto::x509`: the server's chain is walked to one of the compiled-in roots, the host
//! is matched against the SAN names, validity is checked when the clock is set) and the
//! chip's hardware RNG for the key share. Record buffers are the caller's, allocated
//! for one request.

use alloc::vec::Vec;

use embassy_net::tcp::TcpSocket;
use embassy_time::{with_timeout, Duration};
use embedded_tls::{
    Aes128GcmSha256, CertificateEntryRef, CertificateRef, CertificateVerifyRef, CryptoProvider, CryptoRngCore, SignatureScheme,
    TlsCipherSuite, TlsConfig, TlsConnection, TlsContext, TlsError, TlsVerifier,
};
use sha2::Digest;

use crate::proto::rsa::Hash;
use crate::proto::x509::{self, Key, SigAlg};

/// The read record buffer: one full TLS record plus overhead (servers rarely honour a
/// smaller `max_fragment_length`, so this is the safe size).
pub const READ_BUF: usize = 16 * 1024 + 256;
/// The write buffer: requests are small, so records are too.
pub const WRITE_BUF: usize = 2048 + 128;
/// How long the handshake may take.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(25);

/// The chip's hardware RNG as the TLS entropy source.
pub struct HwRng(esp_hal::rng::Rng);

impl HwRng {
    /// A handle on the RNG.
    pub fn new() -> Self {
        HwRng(esp_hal::rng::Rng::new())
    }
}

impl Default for HwRng {
    fn default() -> Self {
        Self::new()
    }
}

impl rand_core::RngCore for HwRng {
    fn next_u32(&mut self) -> u32 {
        self.0.random()
    }
    fn next_u64(&mut self) -> u64 {
        ((self.0.random() as u64) << 32) | self.0.random() as u64
    }
    fn fill_bytes(&mut self, dest: &mut [u8]) {
        for chunk in dest.chunks_mut(4) {
            let w = self.0.random().to_le_bytes();
            chunk.copy_from_slice(&w[..chunk.len()]);
        }
    }
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}

impl rand_core::CryptoRng for HwRng {}

/// The current time (UTC seconds) from the RTC the main loop publishes, or `None`
/// while the clock is unset (before 2024), in which case validity is not checked.
pub fn now_utc() -> Option<u64> {
    let (local, tz) = crate::with(|i| (i.status.local_now, i.tz_minutes));
    if local < 1_704_067_200 {
        return None;
    }
    Some((local as i64 - tz as i64 * 60).max(0) as u64)
}

/// The verifier: chain and host at the Certificate message, the handshake signature at
/// CertificateVerify.
pub struct Verifier {
    host: alloc::string::String,
    leaf: Option<Key>,
    transcript: Option<<Aes128GcmSha256 as TlsCipherSuite>::Hash>,
}

impl Verifier {
    fn new() -> Self {
        Verifier { host: alloc::string::String::new(), leaf: None, transcript: None }
    }
}

impl TlsVerifier<Aes128GcmSha256> for Verifier {
    fn set_hostname_verification(&mut self, hostname: &str) -> Result<(), TlsError> {
        self.host = alloc::string::String::from(hostname);
        Ok(())
    }

    fn verify_certificate(&mut self, transcript: &<Aes128GcmSha256 as TlsCipherSuite>::Hash, cert: CertificateRef) -> Result<(), TlsError> {
        let entries: Vec<&[u8]> = cert
            .entries
            .iter()
            .filter_map(|e| match e {
                CertificateEntryRef::X509(d) => Some(*d),
                CertificateEntryRef::RawPublicKey(_) => None,
            })
            .collect();
        match x509::verify_chain(&entries, &self.host, now_utc(), x509::ROOTS) {
            Ok(key) => {
                self.leaf = Some(key);
                self.transcript = Some(transcript.clone());
                Ok(())
            }
            Err(e) => {
                log::warn!("tls: {} rejected: {e:?}", self.host);
                Err(TlsError::InvalidCertificate)
            }
        }
    }

    fn verify_signature(&mut self, verify: CertificateVerifyRef) -> Result<(), TlsError> {
        let transcript = self.transcript.take().ok_or(TlsError::InvalidSignature)?;
        let key = self.leaf.as_ref().ok_or(TlsError::InvalidSignature)?;
        let mut msg = Vec::with_capacity(64 + 34 + 32);
        msg.resize(64, 0x20);
        msg.extend_from_slice(b"TLS 1.3, server CertificateVerify\0");
        msg.extend_from_slice(&transcript.finalize());
        let r = match verify.signature_scheme {
            SignatureScheme::EcdsaSecp256r1Sha256 => x509::verify(key, SigAlg::Ecdsa(Hash::Sha256), &msg, verify.signature),
            SignatureScheme::EcdsaSecp384r1Sha384 => x509::verify(key, SigAlg::Ecdsa(Hash::Sha384), &msg, verify.signature),
            SignatureScheme::RsaPssRsaeSha256 => x509::verify_pss(key, Hash::Sha256, &msg, verify.signature),
            SignatureScheme::RsaPssRsaeSha384 => x509::verify_pss(key, Hash::Sha384, &msg, verify.signature),
            SignatureScheme::RsaPssRsaeSha512 => x509::verify_pss(key, Hash::Sha512, &msg, verify.signature),
            _ => return Err(TlsError::InvalidSignatureScheme),
        };
        r.map_err(|e| {
            log::warn!("tls: handshake signature: {e:?}");
            TlsError::InvalidSignature
        })
    }
}

/// The crypto provider: hardware RNG and the verifier above.
pub struct Provider {
    rng: HwRng,
    verifier: Verifier,
}

impl Provider {
    /// A provider for one connection.
    pub fn new() -> Self {
        Provider { rng: HwRng::new(), verifier: Verifier::new() }
    }
}

impl Default for Provider {
    fn default() -> Self {
        Self::new()
    }
}

impl CryptoProvider for Provider {
    type CipherSuite = Aes128GcmSha256;
    // Only client certificates would sign with this; Quire presents none.
    type Signature = Vec<u8>;

    fn rng(&mut self) -> impl CryptoRngCore {
        &mut self.rng
    }

    fn verifier(&mut self) -> Result<&mut impl TlsVerifier<Self::CipherSuite>, TlsError> {
        Ok(&mut self.verifier)
    }
}

/// A TLS connection over a socket, for `host`.
pub type Tls<'a> = TlsConnection<'a, TcpSocket<'a>, Aes128GcmSha256>;

/// Run the handshake with `host` over a connected socket. `read_buf` must hold a full
/// record ([`READ_BUF`]); `write_buf` at least [`WRITE_BUF`].
pub async fn open<'a>(socket: TcpSocket<'a>, host: &str, read_buf: &'a mut [u8], write_buf: &'a mut [u8]) -> Result<Tls<'a>, TlsError> {
    let config = TlsConfig::new().with_server_name(host);
    let mut conn = TlsConnection::new(socket, read_buf, write_buf);
    match with_timeout(HANDSHAKE_TIMEOUT, conn.open(TlsContext::new(&config, Provider::new()))).await {
        Ok(Ok(())) => Ok(conn),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(TlsError::Io(embedded_io_async::ErrorKind::TimedOut)),
    }
}
