//! Just enough X.509 to verify a TLS server: a DER walker, the certificate fields path
//! building needs, the root store (`roots/roots.bin`), chain verification and host name
//! matching (RFC 6125: SAN dNSNames with one left-most wildcard label, CN as the
//! fallback). Signatures are RSA PKCS#1 v1.5 and ECDSA P-256/P-384 over SHA-256/384/512.
//!
//! Limits, by design: no name constraints, no revocation, no policy checks, no path
//! length checks beyond `cA`, no Ed25519, at most six certificates per chain.

use alloc::string::String;
use alloc::vec::Vec;

use super::ec;
use super::rsa::{self, Hash};

/// The trust store: subject and SubjectPublicKeyInfo of each root (see `roots/build.py`).
pub const ROOTS: &[u8] = include_bytes!("../../roots/roots.bin");

/// Why a chain was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CertError {
    /// A certificate could not be parsed.
    Malformed,
    /// A key type or signature algorithm this verifier does not do.
    Unsupported,
    /// No trusted root signs the chain.
    Untrusted,
    /// A signature in the chain does not verify.
    BadSignature,
    /// A certificate is not valid now.
    Expired,
    /// The leaf does not name the host.
    HostMismatch,
    /// An intermediate is not a CA.
    NotCa,
}

/// A public key, borrowed from its certificate or copied out of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key {
    /// RSA: modulus and exponent, big-endian.
    Rsa {
        /// Modulus.
        n: Vec<u8>,
        /// Public exponent.
        e: Vec<u8>,
    },
    /// ECDSA on P-256: an uncompressed SEC1 point.
    P256(Vec<u8>),
    /// ECDSA on P-384: an uncompressed SEC1 point.
    P384(Vec<u8>),
}

/// A signature algorithm named by a certificate (or the hash of a TLS scheme).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SigAlg {
    /// RSA PKCS#1 v1.5.
    RsaPkcs1(Hash),
    /// ECDSA with the given hash.
    Ecdsa(Hash),
}

// OIDs, DER-encoded contents.
const OID_RSA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01];
const OID_EC: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01];
const OID_P256: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07];
const OID_P384: &[u8] = &[0x2b, 0x81, 0x04, 0x00, 0x22];
const OID_SHA256_RSA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0b];
const OID_SHA384_RSA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0c];
const OID_SHA512_RSA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0d];
const OID_ECDSA_SHA256: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02];
const OID_ECDSA_SHA384: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x03];
const OID_ECDSA_SHA512: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x04];
const OID_CN: &[u8] = &[0x55, 0x04, 0x03];
const OID_SAN: &[u8] = &[0x55, 0x1d, 0x11];
const OID_BASIC_CONSTRAINTS: &[u8] = &[0x55, 0x1d, 0x13];

const TAG_SEQUENCE: u8 = 0x30;
const TAG_INTEGER: u8 = 0x02;
const TAG_BIT_STRING: u8 = 0x03;
const TAG_OCTET_STRING: u8 = 0x04;
const TAG_OID: u8 = 0x06;
const TAG_UTC_TIME: u8 = 0x17;
const TAG_GENERALIZED_TIME: u8 = 0x18;
const TAG_BOOLEAN: u8 = 0x01;

/// A cursor over DER elements.
#[derive(Clone, Copy)]
pub struct Der<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Der<'a> {
    /// Over `data`.
    pub fn new(data: &'a [u8]) -> Self {
        Der { data, pos: 0 }
    }
    /// Whether everything was consumed.
    pub fn is_empty(&self) -> bool {
        self.pos >= self.data.len()
    }
    /// The tag of the next element, if any.
    pub fn peek_tag(&self) -> Option<u8> {
        self.data.get(self.pos).copied()
    }
    /// Read the next element: (tag, contents, whole element including its header).
    pub fn read(&mut self) -> Option<(u8, &'a [u8], &'a [u8])> {
        let start = self.pos;
        let tag = *self.data.get(self.pos)?;
        let first = *self.data.get(self.pos + 1)?;
        let (len, hdr) = if first & 0x80 == 0 {
            (first as usize, 2)
        } else {
            let n = (first & 0x7f) as usize;
            if n == 0 || n > 4 {
                return None;
            }
            let mut len = 0usize;
            for i in 0..n {
                len = (len << 8) | *self.data.get(self.pos + 2 + i)? as usize;
            }
            (len, 2 + n)
        };
        let body_start = start + hdr;
        let end = body_start.checked_add(len)?;
        if end > self.data.len() {
            return None;
        }
        self.pos = end;
        Some((tag, &self.data[body_start..end], &self.data[start..end]))
    }
    /// The next element, which must have `tag`; returns its contents.
    pub fn expect(&mut self, tag: u8) -> Option<&'a [u8]> {
        let (t, body, _) = self.read()?;
        (t == tag).then_some(body)
    }
    /// Skip an optional context-specific element with the given tag.
    fn skip_optional(&mut self, tag: u8) {
        if self.peek_tag() == Some(tag) {
            let _ = self.read();
        }
    }
}

/// The fields of one certificate this verifier uses. Borrows the DER.
#[derive(Clone, Copy, Debug)]
pub struct Cert<'a> {
    /// The whole `tbsCertificate` element (what the signature is over).
    pub tbs: &'a [u8],
    /// The issuer `Name` element.
    pub issuer: &'a [u8],
    /// The subject `Name` element.
    pub subject: &'a [u8],
    /// Validity, seconds since the Unix epoch.
    pub not_before: u64,
    /// Validity end.
    pub not_after: u64,
    /// The `SubjectPublicKeyInfo` element.
    pub spki: &'a [u8],
    /// The signature algorithm.
    pub sig_alg: Option<SigAlg>,
    /// The signature bytes.
    pub signature: &'a [u8],
    /// The `SubjectAltName` extension's `GeneralNames` contents, when present.
    pub san: Option<&'a [u8]>,
    /// The `cA` flag of basic constraints (`None` without the extension).
    pub is_ca: Option<bool>,
}

fn alg_oid(alg: &[u8]) -> Option<(&[u8], Option<&[u8]>)> {
    let mut d = Der::new(alg);
    let oid = d.expect(TAG_OID)?;
    let params = d.read().map(|(_, body, whole)| if whole.first() == Some(&TAG_OID) { body } else { whole });
    Some((oid, params))
}

fn sig_alg(alg: &[u8]) -> Option<SigAlg> {
    let (oid, _) = alg_oid(alg)?;
    Some(match oid {
        OID_SHA256_RSA => SigAlg::RsaPkcs1(Hash::Sha256),
        OID_SHA384_RSA => SigAlg::RsaPkcs1(Hash::Sha384),
        OID_SHA512_RSA => SigAlg::RsaPkcs1(Hash::Sha512),
        OID_ECDSA_SHA256 => SigAlg::Ecdsa(Hash::Sha256),
        OID_ECDSA_SHA384 => SigAlg::Ecdsa(Hash::Sha384),
        OID_ECDSA_SHA512 => SigAlg::Ecdsa(Hash::Sha512),
        _ => return None,
    })
}

fn two_digits(b: &[u8]) -> Option<u64> {
    if b.len() < 2 || !b[0].is_ascii_digit() || !b[1].is_ascii_digit() {
        return None;
    }
    Some(((b[0] - b'0') as u64) * 10 + (b[1] - b'0') as u64)
}

/// Days since 1970-01-01 of a civil date (proleptic Gregorian).
fn days_from_civil(y: i64, m: u64, d: u64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe as i64 - 719468
}

/// Parse a `UTCTime` (`YYMMDDHHMMSSZ`) or `GeneralizedTime` (`YYYYMMDDHHMMSSZ`).
pub fn parse_time(tag: u8, b: &[u8]) -> Option<u64> {
    let (year, rest) = match tag {
        TAG_UTC_TIME => {
            let yy = two_digits(b)?;
            (if yy >= 50 { 1900 + yy } else { 2000 + yy }, &b[2..])
        }
        TAG_GENERALIZED_TIME => (two_digits(b)? * 100 + two_digits(b.get(2..)?)?, &b[4..]),
        _ => return None,
    };
    if rest.len() < 10 || rest[rest.len() - 1] != b'Z' {
        return None;
    }
    let (mo, d, h, mi, s) =
        (two_digits(rest)?, two_digits(&rest[2..])?, two_digits(&rest[4..])?, two_digits(&rest[6..])?, two_digits(&rest[8..])?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || s > 60 {
        return None;
    }
    let days = days_from_civil(year as i64, mo, d);
    if days < 0 {
        return None;
    }
    Some(days as u64 * 86400 + h * 3600 + mi * 60 + s)
}

impl<'a> Cert<'a> {
    /// Parse a DER certificate.
    pub fn parse(der: &'a [u8]) -> Option<Cert<'a>> {
        let mut top = Der::new(der);
        let cert = top.expect(TAG_SEQUENCE)?;
        let mut c = Der::new(cert);
        let (t, tbs_body, tbs) = c.read()?;
        if t != TAG_SEQUENCE {
            return None;
        }
        let alg = c.expect(TAG_SEQUENCE)?;
        let sig_bits = c.expect(TAG_BIT_STRING)?;
        if sig_bits.first() != Some(&0) {
            return None;
        }
        let signature = &sig_bits[1..];
        let sig_alg = sig_alg(alg);
        let mut f = Der::new(tbs_body);
        f.skip_optional(0xa0);
        f.expect(TAG_INTEGER)?;
        let inner_alg = f.expect(TAG_SEQUENCE)?;
        if alg_oid(inner_alg)?.0 != alg_oid(alg)?.0 {
            return None;
        }
        let (_, _, issuer) = f.read()?;
        let validity = f.expect(TAG_SEQUENCE)?;
        let mut v = Der::new(validity);
        let (t1, b1, _) = v.read()?;
        let (t2, b2, _) = v.read()?;
        let (not_before, not_after) = (parse_time(t1, b1)?, parse_time(t2, b2)?);
        let (_, _, subject) = f.read()?;
        let (_, _, spki) = f.read()?;
        f.skip_optional(0x81);
        f.skip_optional(0x82);
        let mut san = None;
        let mut is_ca = None;
        if f.peek_tag() == Some(0xa3) {
            let (_, ext_wrap, _) = f.read()?;
            let exts = Der::new(ext_wrap).expect(TAG_SEQUENCE)?;
            let mut e = Der::new(exts);
            while !e.is_empty() {
                let ext = e.expect(TAG_SEQUENCE)?;
                let mut x = Der::new(ext);
                let oid = x.expect(TAG_OID)?;
                if x.peek_tag() == Some(TAG_BOOLEAN) {
                    let _ = x.read();
                }
                let value = x.expect(TAG_OCTET_STRING)?;
                match oid {
                    OID_SAN => san = Some(Der::new(value).expect(TAG_SEQUENCE)?),
                    OID_BASIC_CONSTRAINTS => {
                        let bc = Der::new(value).expect(TAG_SEQUENCE)?;
                        let mut b = Der::new(bc);
                        is_ca = Some(match b.read() {
                            Some((TAG_BOOLEAN, body, _)) => body.first().is_some_and(|v| *v != 0),
                            _ => false,
                        });
                    }
                    _ => {}
                }
            }
        }
        Some(Cert { tbs, issuer, subject, not_before, not_after, spki, sig_alg, signature, san, is_ca })
    }

    /// The public key.
    pub fn key(&self) -> Result<Key, CertError> {
        key_of(self.spki)
    }

    /// Whether the certificate names `host` (SAN dNSNames, else the CN).
    pub fn matches_host(&self, host: &str) -> bool {
        let host = host.trim_end_matches('.');
        if let Some(san) = self.san {
            let mut d = Der::new(san);
            let mut any_dns = false;
            while let Some((tag, body, _)) = d.read() {
                if tag == 0x82 {
                    any_dns = true;
                    if let Ok(name) = core::str::from_utf8(body) {
                        if name_matches(name, host) {
                            return true;
                        }
                    }
                }
            }
            if any_dns {
                return false;
            }
        }
        self.common_name().is_some_and(|cn| name_matches(&cn, host))
    }

    /// The subject's CN, for logs and the CN fallback.
    pub fn common_name(&self) -> Option<String> {
        let mut rdns = Der::new(Der::new(self.subject).expect(TAG_SEQUENCE)?);
        let mut out = None;
        while let Some((_, set, _)) = rdns.read() {
            let mut s = Der::new(set);
            while let Some((_, atv, _)) = s.read() {
                let mut a = Der::new(atv);
                if a.expect(TAG_OID) == Some(OID_CN) {
                    if let Some((_, v, _)) = a.read() {
                        out = core::str::from_utf8(v).ok().map(String::from);
                    }
                }
            }
        }
        out
    }
}

/// The key inside a `SubjectPublicKeyInfo` element.
pub fn key_of(spki: &[u8]) -> Result<Key, CertError> {
    let mut d = Der::new(spki);
    let body = d.expect(TAG_SEQUENCE).ok_or(CertError::Malformed)?;
    let mut s = Der::new(body);
    let alg = s.expect(TAG_SEQUENCE).ok_or(CertError::Malformed)?;
    let bits = s.expect(TAG_BIT_STRING).ok_or(CertError::Malformed)?;
    if bits.first() != Some(&0) {
        return Err(CertError::Malformed);
    }
    let raw = &bits[1..];
    let (oid, params) = alg_oid(alg).ok_or(CertError::Malformed)?;
    match oid {
        OID_RSA => {
            let seq = Der::new(raw).expect(TAG_SEQUENCE).ok_or(CertError::Malformed)?;
            let mut r = Der::new(seq);
            let n = r.expect(TAG_INTEGER).ok_or(CertError::Malformed)?;
            let e = r.expect(TAG_INTEGER).ok_or(CertError::Malformed)?;
            Ok(Key::Rsa { n: n.to_vec(), e: e.to_vec() })
        }
        OID_EC => match params {
            Some(OID_P256) if raw.len() == 65 => Ok(Key::P256(raw.to_vec())),
            Some(OID_P384) if raw.len() == 97 => Ok(Key::P384(raw.to_vec())),
            _ => Err(CertError::Unsupported),
        },
        _ => Err(CertError::Unsupported),
    }
}

/// RFC 6125 §6.4.3 matching: case-insensitive, one wildcard as the whole left-most
/// label, at least two labels after it.
pub fn name_matches(pattern: &str, host: &str) -> bool {
    let pattern = pattern.trim_end_matches('.');
    if pattern.is_empty() || host.is_empty() {
        return false;
    }
    if let Some(rest) = pattern.strip_prefix("*.") {
        if rest.contains('*') || rest.matches('.').count() < 1 {
            return false;
        }
        let Some((first, tail)) = host.split_once('.') else { return false };
        return !first.is_empty() && !first.contains('*') && tail.eq_ignore_ascii_case(rest);
    }
    !pattern.contains('*') && pattern.eq_ignore_ascii_case(host)
}

/// Verify `signature` over `data` with `alg` and `key`.
pub fn verify(key: &Key, alg: SigAlg, data: &[u8], signature: &[u8]) -> Result<(), CertError> {
    match (key, alg) {
        (Key::Rsa { n, e }, SigAlg::RsaPkcs1(h)) => {
            let digest = h.digest(&[data]);
            rsa::verify_pkcs1v15(rsa::PublicKey { n, e }, h, &digest, signature).then_some(()).ok_or(CertError::BadSignature)
        }
        (Key::P256(point), SigAlg::Ecdsa(h)) => {
            let digest = h.digest(&[data]);
            ec::verify(ec::Curve::P256, point, &digest, signature).then_some(()).ok_or(CertError::BadSignature)
        }
        (Key::P384(point), SigAlg::Ecdsa(h)) => {
            let digest = h.digest(&[data]);
            ec::verify(ec::Curve::P384, point, &digest, signature).then_some(()).ok_or(CertError::BadSignature)
        }
        _ => Err(CertError::Unsupported),
    }
}

/// Verify an RSA-PSS signature (TLS 1.3 `rsa_pss_rsae_*`) over `data`.
pub fn verify_pss(key: &Key, hash: Hash, data: &[u8], signature: &[u8]) -> Result<(), CertError> {
    match key {
        Key::Rsa { n, e } => {
            let digest = hash.digest(&[data]);
            rsa::verify_pss(rsa::PublicKey { n, e }, hash, &digest, signature).then_some(()).ok_or(CertError::BadSignature)
        }
        _ => Err(CertError::Unsupported),
    }
}

/// One root of the store.
pub struct Root<'a> {
    /// Its CN.
    pub name: &'a str,
    /// Its subject `Name` element.
    pub subject: &'a [u8],
    /// Its `SubjectPublicKeyInfo` element.
    pub spki: &'a [u8],
}

/// Iterate the roots of `store` (`ROOTS` by default).
pub fn roots(store: &[u8]) -> impl Iterator<Item = Root<'_>> {
    let mut pos = 0usize;
    core::iter::from_fn(move || {
        let n = *store.get(pos)? as usize;
        let name = core::str::from_utf8(store.get(pos + 1..pos + 1 + n)?).ok()?;
        pos += 1 + n;
        let sl = u16::from_le_bytes([*store.get(pos)?, *store.get(pos + 1)?]) as usize;
        let subject = store.get(pos + 2..pos + 2 + sl)?;
        pos += 2 + sl;
        let kl = u16::from_le_bytes([*store.get(pos)?, *store.get(pos + 1)?]) as usize;
        let spki = store.get(pos + 2..pos + 2 + kl)?;
        pos += 2 + kl;
        Some(Root { name, subject, spki })
    })
}

/// Most certificates walked from the leaf.
const MAX_CHAIN: usize = 6;

/// Verify the chain a server sent (leaf first, in any order after that) for `host` at
/// time `now` (seconds; `None` skips the validity check) against `store`. Returns the
/// leaf's key for the TLS signature check.
pub fn verify_chain(entries: &[&[u8]], host: &str, now: Option<u64>, store: &[u8]) -> Result<Key, CertError> {
    let leaf_der = *entries.first().ok_or(CertError::Malformed)?;
    let leaf = Cert::parse(leaf_der).ok_or(CertError::Malformed)?;
    if !leaf.matches_host(host) {
        return Err(CertError::HostMismatch);
    }
    let leaf_key = leaf.key()?;
    let mut cur = leaf;
    for depth in 0..MAX_CHAIN {
        if let Some(t) = now {
            if t < cur.not_before || t > cur.not_after {
                return Err(CertError::Expired);
            }
        }
        if depth > 0 && cur.is_ca != Some(true) {
            return Err(CertError::NotCa);
        }
        let alg = cur.sig_alg.ok_or(CertError::Unsupported)?;
        // A root that issued this certificate ends the walk.
        if let Some(root) = roots(store).find(|r| r.subject == cur.issuer) {
            let key = key_of(root.spki)?;
            return verify(&key, alg, cur.tbs, cur.signature).map(|_| leaf_key);
        }
        // Else the issuer must be among the sent certificates (self-issued ones skipped).
        let next = entries
            .iter()
            .skip(1)
            .filter_map(|d| Cert::parse(d))
            .find(|c| c.subject == cur.issuer && c.subject != c.issuer && c.tbs != cur.tbs)
            .ok_or(CertError::Untrusted)?;
        let key = key_of(next.spki)?;
        verify(&key, alg, cur.tbs, cur.signature)?;
        cur = next;
    }
    Err(CertError::Untrusted)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fixtures generated by testdata/make.sh (a private test CA, never trusted on devices).
    const ROOT_RSA: &[u8] = include_bytes!("../../testdata/ca-rsa.der");
    const INTER_RSA: &[u8] = include_bytes!("../../testdata/inter-rsa.der");
    const LEAF_P256: &[u8] = include_bytes!("../../testdata/leaf-p256.der");
    const ROOT_P384: &[u8] = include_bytes!("../../testdata/ca-p384.der");
    const LEAF_P384: &[u8] = include_bytes!("../../testdata/leaf-p384-chain.der");
    const LEAF_RSA: &[u8] = include_bytes!("../../testdata/leaf-rsa.der");
    const TEST_STORE: &[u8] = include_bytes!("../../testdata/roots.bin");
    /// 2026-10-01T00:00:00Z, inside every fixture's validity (leaves: one year from the
    /// day make.sh ran, 2026-09-15).
    const NOW: u64 = 1_790_812_800;

    #[test]
    fn parses_real_roots() {
        let mut n = 0;
        for r in roots(ROOTS) {
            let key = key_of(r.spki).unwrap();
            assert!(matches!(key, Key::Rsa { .. } | Key::P256(_) | Key::P384(_)), "{}", r.name);
            assert_eq!(Der::new(r.subject).peek_tag(), Some(TAG_SEQUENCE));
            n += 1;
        }
        assert!(n >= 12);
        assert!(roots(ROOTS).any(|r| r.name == "ISRG Root X1"));
    }

    #[test]
    fn parses_fixture_certificates() {
        let c = Cert::parse(LEAF_P256).unwrap();
        assert_eq!(c.common_name().as_deref(), Some("books.example.test"));
        assert!(c.matches_host("books.example.test"));
        assert!(c.matches_host("api.example.test"));
        assert!(c.matches_host("API.EXAMPLE.TEST"));
        assert!(!c.matches_host("a.b.example.test"));
        assert!(!c.matches_host("example.test"));
        assert!(c.not_before < NOW && NOW < c.not_after);
        assert_eq!(c.sig_alg, Some(SigAlg::RsaPkcs1(Hash::Sha256)));
        assert!(matches!(c.key().unwrap(), Key::P256(_)));
        let i = Cert::parse(INTER_RSA).unwrap();
        assert_eq!(i.is_ca, Some(true));
        assert!(matches!(i.key().unwrap(), Key::Rsa { .. }));
    }

    #[test]
    fn chain_through_intermediate_to_rsa_root() {
        assert!(matches!(verify_chain(&[LEAF_P256, INTER_RSA], "books.example.test", Some(NOW), TEST_STORE), Ok(Key::P256(_))));
        // Order after the leaf does not matter; the root itself may be sent too.
        assert!(verify_chain(&[LEAF_P256, ROOT_RSA, INTER_RSA], "api.example.test", Some(NOW), TEST_STORE).is_ok());
        assert_eq!(verify_chain(&[LEAF_P256], "books.example.test", Some(NOW), TEST_STORE), Err(CertError::Untrusted));
        assert_eq!(verify_chain(&[LEAF_P256, INTER_RSA], "books.example.org", Some(NOW), TEST_STORE), Err(CertError::HostMismatch));
        assert_eq!(
            verify_chain(&[LEAF_P256, INTER_RSA], "books.example.test", Some(NOW + 400 * 86400), TEST_STORE),
            Err(CertError::Expired)
        );
        assert_eq!(verify_chain(&[LEAF_P256, INTER_RSA], "books.example.test", Some(NOW), ROOTS), Err(CertError::Untrusted));
        assert!(verify_chain(&[LEAF_P256, INTER_RSA], "books.example.test", None, TEST_STORE).is_ok());
    }

    #[test]
    fn chain_to_p384_root_and_rsa_leaf() {
        assert!(matches!(verify_chain(&[LEAF_P384], "ecc.example.test", Some(NOW), TEST_STORE), Ok(Key::P256(_))));
        assert!(matches!(verify_chain(&[LEAF_RSA, INTER_RSA], "rsa.example.test", Some(NOW), TEST_STORE), Ok(Key::Rsa { .. })));
        let mut tampered = LEAF_P384.to_vec();
        let n = tampered.len();
        tampered[n - 5] ^= 0x40;
        assert!(verify_chain(&[&tampered], "ecc.example.test", Some(NOW), TEST_STORE).is_err());
        let _ = ROOT_P384;
    }

    #[test]
    fn tls13_signatures() {
        let leaf = Cert::parse(LEAF_RSA).unwrap().key().unwrap();
        let msg = include_bytes!("../../testdata/tls-msg.bin");
        let sig = include_bytes!("../../testdata/tls-sig-rsa-pss-sha256.bin");
        assert!(verify_pss(&leaf, Hash::Sha256, msg, sig).is_ok());
        assert!(verify_pss(&leaf, Hash::Sha384, msg, sig).is_err());
        let ec = Cert::parse(LEAF_P256).unwrap().key().unwrap();
        let sig = include_bytes!("../../testdata/tls-sig-p256-sha256.bin");
        assert!(verify(&ec, SigAlg::Ecdsa(Hash::Sha256), msg, sig).is_ok());
        assert!(verify(&ec, SigAlg::Ecdsa(Hash::Sha256), b"nope", sig).is_err());
        assert_eq!(verify_pss(&ec, Hash::Sha256, msg, sig), Err(CertError::Unsupported));
    }

    #[test]
    fn times_and_names() {
        assert_eq!(parse_time(TAG_UTC_TIME, b"700101000000Z"), Some(0));
        assert_eq!(parse_time(TAG_GENERALIZED_TIME, b"20261001000000Z"), Some(NOW));
        assert_eq!(parse_time(TAG_UTC_TIME, b"491231235959Z"), Some(2_524_607_999));
        assert_eq!(parse_time(TAG_UTC_TIME, b"7001010000"), None);
        assert!(name_matches("*.wikipedia.org", "en.wikipedia.org"));
        assert!(!name_matches("*.wikipedia.org", "a.en.wikipedia.org"));
        assert!(!name_matches("*.org", "wikipedia.org"));
        assert!(!name_matches("w*.org", "wikipedia.org"));
        assert!(name_matches("Example.COM", "example.com"));
    }
}
