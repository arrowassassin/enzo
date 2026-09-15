//! RSA signature *verification* only (PKCS#1 v1.5 for certificate chains, PSS for the
//! TLS 1.3 CertificateVerify) on the Montgomery kernel in [`super::bignum`]: public
//! exponents are tiny, so one modular exponentiation of a 2048–4096-bit signature is a
//! few dozen multiplications. No secret is ever handled here, so nothing is constant-time.

use alloc::vec::Vec;

use super::bignum::{strip, Modulus};

/// A hash the signatures here can be over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hash {
    /// SHA-256.
    Sha256,
    /// SHA-384.
    Sha384,
    /// SHA-512.
    Sha512,
}

impl Hash {
    /// Digest length in bytes.
    pub fn digest_len(self) -> usize {
        match self {
            Hash::Sha256 => 32,
            Hash::Sha384 => 48,
            Hash::Sha512 => 64,
        }
    }
    /// Hash the concatenation of `parts`.
    pub fn digest(self, parts: &[&[u8]]) -> Vec<u8> {
        use sha2::Digest;
        match self {
            Hash::Sha256 => {
                let mut h = sha2::Sha256::new();
                parts.iter().for_each(|p| h.update(p));
                h.finalize().to_vec()
            }
            Hash::Sha384 => {
                let mut h = sha2::Sha384::new();
                parts.iter().for_each(|p| h.update(p));
                h.finalize().to_vec()
            }
            Hash::Sha512 => {
                let mut h = sha2::Sha512::new();
                parts.iter().for_each(|p| h.update(p));
                h.finalize().to_vec()
            }
        }
    }
    /// The DER `DigestInfo` prefix PKCS#1 v1.5 puts before the digest.
    fn digest_info_prefix(self) -> &'static [u8] {
        match self {
            Hash::Sha256 => {
                &[0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, 0x05, 0x00, 0x04, 0x20]
            }
            Hash::Sha384 => {
                &[0x30, 0x41, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x02, 0x05, 0x00, 0x04, 0x30]
            }
            Hash::Sha512 => {
                &[0x30, 0x51, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x03, 0x05, 0x00, 0x04, 0x40]
            }
        }
    }
}

/// An RSA public key: modulus and exponent as big-endian bytes (leading zeros allowed).
#[derive(Clone, Copy, Debug)]
pub struct PublicKey<'a> {
    /// Modulus.
    pub n: &'a [u8],
    /// Public exponent.
    pub e: &'a [u8],
}

/// Verify a PKCS#1 v1.5 signature over `digest` (already hashed with `hash`).
pub fn verify_pkcs1v15(key: PublicKey, hash: Hash, digest: &[u8], sig: &[u8]) -> bool {
    let Some(m) = Modulus::new(key.n) else { return false };
    let k = m.byte_len();
    if strip(sig).len() > k || digest.len() != hash.digest_len() {
        return false;
    }
    let Some(em) = m.pow_small(sig, key.e) else { return false };
    let em = &em[em.len() - k..];
    let prefix = hash.digest_info_prefix();
    let t_len = prefix.len() + digest.len();
    if k < t_len + 11 || em[0] != 0 || em[1] != 1 {
        return false;
    }
    let ps_end = k - t_len - 1;
    if em[ps_end] != 0 || em[2..ps_end].iter().any(|b| *b != 0xff) {
        return false;
    }
    &em[ps_end + 1..ps_end + 1 + prefix.len()] == prefix && &em[k - digest.len()..] == digest
}

/// MGF1 with `hash`, producing `len` bytes.
fn mgf1(hash: Hash, seed: &[u8], len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len + hash.digest_len());
    let mut counter = 0u32;
    while out.len() < len {
        out.extend_from_slice(&hash.digest(&[seed, &counter.to_be_bytes()]));
        counter += 1;
    }
    out.truncate(len);
    out
}

/// Verify an RSASSA-PSS signature over `digest` (hashed with `hash`, MGF1 with the same
/// hash and a salt as long as the digest, as TLS 1.3 requires).
pub fn verify_pss(key: PublicKey, hash: Hash, digest: &[u8], sig: &[u8]) -> bool {
    let Some(m) = Modulus::new(key.n) else { return false };
    let h_len = hash.digest_len();
    if digest.len() != h_len {
        return false;
    }
    let em_bits = m.bit_len() - 1;
    let em_len = em_bits.div_ceil(8);
    if strip(sig).len() > m.byte_len() {
        return false;
    }
    let Some(em) = m.pow_small(sig, key.e) else { return false };
    let em = &em[em.len() - em_len..];
    if em_len < h_len * 2 + 2 || em[em_len - 1] != 0xbc {
        return false;
    }
    let db_len = em_len - h_len - 1;
    let (masked_db, h) = (&em[..db_len], &em[db_len..em_len - 1]);
    let top_bits = 8 * em_len - em_bits;
    if top_bits > 0 && masked_db[0] >> (8 - top_bits) != 0 {
        return false;
    }
    let mask = mgf1(hash, h, db_len);
    let mut db: Vec<u8> = masked_db.iter().zip(mask.iter()).map(|(a, b)| a ^ b).collect();
    if top_bits > 0 {
        db[0] &= 0xff >> top_bits;
    }
    let ps_len = db_len - h_len - 1;
    if db[..ps_len].iter().any(|b| *b != 0) || db[ps_len] != 1 {
        return false;
    }
    let salt = &db[ps_len + 1..];
    let h2 = hash.digest(&[&[0u8; 8], digest, salt]);
    h2.as_slice() == h
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test vectors produced with OpenSSL (testdata/make.sh).
    const N: &[u8] = include_bytes!("../../testdata/rsa2048-n.bin");
    const E: &[u8] = &[1, 0, 1];
    const MSG: &[u8] = b"quire rsa test message";
    const SIG_V15: &[u8] = include_bytes!("../../testdata/rsa2048-sig-pkcs1-sha256.bin");
    const SIG_PSS: &[u8] = include_bytes!("../../testdata/rsa2048-sig-pss-sha256.bin");

    #[test]
    fn pkcs1v15_roundtrip() {
        let d = Hash::Sha256.digest(&[MSG]);
        assert!(verify_pkcs1v15(PublicKey { n: N, e: E }, Hash::Sha256, &d, SIG_V15));
        let mut bad = SIG_V15.to_vec();
        bad[10] ^= 1;
        assert!(!verify_pkcs1v15(PublicKey { n: N, e: E }, Hash::Sha256, &d, &bad));
        assert!(!verify_pkcs1v15(PublicKey { n: N, e: E }, Hash::Sha384, &Hash::Sha384.digest(&[MSG]), SIG_V15));
    }

    #[test]
    fn pss_roundtrip() {
        let d = Hash::Sha256.digest(&[MSG]);
        assert!(verify_pss(PublicKey { n: N, e: E }, Hash::Sha256, &d, SIG_PSS));
        let d2 = Hash::Sha256.digest(&[b"other"]);
        assert!(!verify_pss(PublicKey { n: N, e: E }, Hash::Sha256, &d2, SIG_PSS));
        assert!(!verify_pss(PublicKey { n: N, e: E }, Hash::Sha256, &d, SIG_V15));
    }

    #[test]
    fn rejects_odd_keys() {
        assert!(!verify_pkcs1v15(PublicKey { n: &[0u8; 256], e: E }, Hash::Sha256, &[0u8; 32], SIG_V15));
        assert!(!verify_pkcs1v15(PublicKey { n: N, e: &[1, 2, 3, 4, 5] }, Hash::Sha256, &[0u8; 32], SIG_V15));
        assert!(!verify_pkcs1v15(PublicKey { n: N, e: E }, Hash::Sha256, &[0u8; 32], &[0xff; 300]));
    }
}
