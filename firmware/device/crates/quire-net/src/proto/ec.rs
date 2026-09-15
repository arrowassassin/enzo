//! ECDSA *verification* on P-256 and P-384 over the Montgomery kernel in
//! [`super::bignum`]: Jacobian point arithmetic (a = −3), double-and-add scalar
//! multiplication, Fermat inverses. A verification takes a few hundred milliseconds on
//! the device and happens once or twice per TLS connection, which buys a code footprint
//! about a tenth of the generic curve crates'. No secrets, no constant time.

use alloc::vec::Vec;

use super::bignum::{is_zero, to_be, Limbs, Modulus};

/// A curve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Curve {
    /// secp256r1.
    P256,
    /// secp384r1.
    P384,
}

struct Params {
    p: &'static [u8],
    n: &'static [u8],
    b: &'static [u8],
    gx: &'static [u8],
    gy: &'static [u8],
    size: usize,
}

const P256: Params = Params {
    p: &[
        0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff,
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    ],
    n: &[
        0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xbc, 0xe6, 0xfa, 0xad, 0xa7, 0x17,
        0x9e, 0x84, 0xf3, 0xb9, 0xca, 0xc2, 0xfc, 0x63, 0x25, 0x51,
    ],
    b: &[
        0x5a, 0xc6, 0x35, 0xd8, 0xaa, 0x3a, 0x93, 0xe7, 0xb3, 0xeb, 0xbd, 0x55, 0x76, 0x98, 0x86, 0xbc, 0x65, 0x1d, 0x06, 0xb0, 0xcc, 0x53,
        0xb0, 0xf6, 0x3b, 0xce, 0x3c, 0x3e, 0x27, 0xd2, 0x60, 0x4b,
    ],
    gx: &[
        0x6b, 0x17, 0xd1, 0xf2, 0xe1, 0x2c, 0x42, 0x47, 0xf8, 0xbc, 0xe6, 0xe5, 0x63, 0xa4, 0x40, 0xf2, 0x77, 0x03, 0x7d, 0x81, 0x2d, 0xeb,
        0x33, 0xa0, 0xf4, 0xa1, 0x39, 0x45, 0xd8, 0x98, 0xc2, 0x96,
    ],
    gy: &[
        0x4f, 0xe3, 0x42, 0xe2, 0xfe, 0x1a, 0x7f, 0x9b, 0x8e, 0xe7, 0xeb, 0x4a, 0x7c, 0x0f, 0x9e, 0x16, 0x2b, 0xce, 0x33, 0x57, 0x6b, 0x31,
        0x5e, 0xce, 0xcb, 0xb6, 0x40, 0x68, 0x37, 0xbf, 0x51, 0xf5,
    ],
    size: 32,
};

const P384: Params = Params {
    p: &[
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0xff, 0xff, 0xff, 0xff,
    ],
    n: &[
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0xff, 0xc7, 0x63, 0x4d, 0x81, 0xf4, 0x37, 0x2d, 0xdf, 0x58, 0x1a, 0x0d, 0xb2, 0x48, 0xb0, 0xa7, 0x7a, 0xec, 0xec, 0x19, 0x6a,
        0xcc, 0xc5, 0x29, 0x73,
    ],
    b: &[
        0xb3, 0x31, 0x2f, 0xa7, 0xe2, 0x3e, 0xe7, 0xe4, 0x98, 0x8e, 0x05, 0x6b, 0xe3, 0xf8, 0x2d, 0x19, 0x18, 0x1d, 0x9c, 0x6e, 0xfe, 0x81,
        0x41, 0x12, 0x03, 0x14, 0x08, 0x8f, 0x50, 0x13, 0x87, 0x5a, 0xc6, 0x56, 0x39, 0x8d, 0x8a, 0x2e, 0xd1, 0x9d, 0x2a, 0x85, 0xc8, 0xed,
        0xd3, 0xec, 0x2a, 0xef,
    ],
    gx: &[
        0xaa, 0x87, 0xca, 0x22, 0xbe, 0x8b, 0x05, 0x37, 0x8e, 0xb1, 0xc7, 0x1e, 0xf3, 0x20, 0xad, 0x74, 0x6e, 0x1d, 0x3b, 0x62, 0x8b, 0xa7,
        0x9b, 0x98, 0x59, 0xf7, 0x41, 0xe0, 0x82, 0x54, 0x2a, 0x38, 0x55, 0x02, 0xf2, 0x5d, 0xbf, 0x55, 0x29, 0x6c, 0x3a, 0x54, 0x5e, 0x38,
        0x72, 0x76, 0x0a, 0xb7,
    ],
    gy: &[
        0x36, 0x17, 0xde, 0x4a, 0x96, 0x26, 0x2c, 0x6f, 0x5d, 0x9e, 0x98, 0xbf, 0x92, 0x92, 0xdc, 0x29, 0xf8, 0xf4, 0x1d, 0xbd, 0x28, 0x9a,
        0x14, 0x7c, 0xe9, 0xda, 0x31, 0x13, 0xb5, 0xf0, 0xb8, 0xc0, 0x0a, 0x60, 0xb1, 0xce, 0x1d, 0x7e, 0x81, 0x9d, 0x7a, 0x43, 0x1d, 0x7c,
        0x90, 0xea, 0x0e, 0x5f,
    ],
    size: 48,
};

/// A Jacobian point in Montgomery form; `z == 0` is the point at infinity.
struct Point {
    x: Limbs,
    y: Limbs,
    z: Limbs,
}

struct Field {
    f: Modulus,
    /// 3 in Montgomery form (a = −3).
    three: Limbs,
}

impl Field {
    fn mul(&self, a: &[u32], b: &[u32]) -> Limbs {
        self.f.mont_mul(a, b)
    }
    fn sqr(&self, a: &[u32]) -> Limbs {
        self.f.mont_mul(a, a)
    }
    fn add(&self, a: &[u32], b: &[u32]) -> Limbs {
        self.f.add(a, b)
    }
    fn sub(&self, a: &[u32], b: &[u32]) -> Limbs {
        self.f.sub(a, b)
    }
    fn dbl_(&self, a: &[u32]) -> Limbs {
        self.f.add(a, a)
    }

    /// 2P.
    fn double(&self, p: &Point) -> Point {
        if is_zero(&p.z) {
            return Point { x: p.x.clone(), y: p.y.clone(), z: p.z.clone() };
        }
        // dbl-2001-b for a = -3.
        let delta = self.sqr(&p.z);
        let gamma = self.sqr(&p.y);
        let beta = self.mul(&p.x, &gamma);
        let alpha = {
            let t = self.mul(&self.sub(&p.x, &delta), &self.add(&p.x, &delta));
            self.mul(&self.three, &t)
        };
        let beta4 = self.dbl_(&self.dbl_(&beta));
        let beta8 = self.dbl_(&beta4);
        let x3 = self.sub(&self.sqr(&alpha), &beta8);
        let z3 = {
            let t = self.sqr(&self.add(&p.y, &p.z));
            self.sub(&self.sub(&t, &gamma), &delta)
        };
        let y3 = {
            let g2 = self.sqr(&gamma);
            let g8 = self.dbl_(&self.dbl_(&self.dbl_(&g2)));
            self.sub(&self.mul(&alpha, &self.sub(&beta4, &x3)), &g8)
        };
        Point { x: x3, y: y3, z: z3 }
    }

    /// P + Q.
    fn add_points(&self, p: &Point, q: &Point) -> Point {
        if is_zero(&p.z) {
            return Point { x: q.x.clone(), y: q.y.clone(), z: q.z.clone() };
        }
        if is_zero(&q.z) {
            return Point { x: p.x.clone(), y: p.y.clone(), z: p.z.clone() };
        }
        let z1z1 = self.sqr(&p.z);
        let z2z2 = self.sqr(&q.z);
        let u1 = self.mul(&p.x, &z2z2);
        let u2 = self.mul(&q.x, &z1z1);
        let s1 = self.mul(&p.y, &self.mul(&q.z, &z2z2));
        let s2 = self.mul(&q.y, &self.mul(&p.z, &z1z1));
        let h = self.sub(&u2, &u1);
        let r = self.sub(&s2, &s1);
        if is_zero(&h) {
            if is_zero(&r) {
                return self.double(p);
            }
            let zero = alloc::vec![0u32; self.f.limbs()];
            return Point { x: self.f.one_mont(), y: self.f.one_mont(), z: zero };
        }
        let hh = self.sqr(&h);
        let hhh = self.mul(&h, &hh);
        let v = self.mul(&u1, &hh);
        let x3 = self.sub(&self.sub(&self.sqr(&r), &hhh), &self.dbl_(&v));
        let y3 = self.sub(&self.mul(&r, &self.sub(&v, &x3)), &self.mul(&s1, &hhh));
        let z3 = self.mul(&self.mul(&p.z, &q.z), &h);
        Point { x: x3, y: y3, z: z3 }
    }

    /// k·P for a big-endian scalar.
    fn mul_point(&self, k: &[u8], p: &Point) -> Point {
        let zero = alloc::vec![0u32; self.f.limbs()];
        let mut acc = Point { x: self.f.one_mont(), y: self.f.one_mont(), z: zero };
        for byte in k {
            for bit in (0..8).rev() {
                acc = self.double(&acc);
                if (byte >> bit) & 1 == 1 {
                    acc = self.add_points(&acc, p);
                }
            }
        }
        acc
    }

    /// The affine x of a point, as bytes; `None` for infinity.
    fn affine_x(&self, p: &Point) -> Option<Vec<u8>> {
        if is_zero(&p.z) {
            return None;
        }
        let zinv = self.f.pow_mont(&p.z, &self.f.minus_two_be());
        let x = self.mul(&p.x, &self.sqr(&zinv));
        Some(to_be(&self.f.from_mont(&x)))
    }
}

/// One non-negative DER INTEGER's content at the front of `d`, strictly encoded (no
/// sign bit, no superfluous leading zero), as DER and RFC 5480 §2.2 demand.
fn der_int(d: &[u8]) -> Option<(&[u8], &[u8])> {
    if d.first() != Some(&0x02) {
        return None;
    }
    let len = *d.get(1)? as usize;
    if len == 0 || len >= 0x80 {
        return None;
    }
    let v = d.get(2..2 + len)?;
    if v[0] & 0x80 != 0 || (v[0] == 0 && v.get(1).is_some_and(|b| b & 0x80 == 0)) {
        return None;
    }
    Some((v, &d[2 + len..]))
}

/// r and s of a DER-encoded `ECDSA-Sig-Value`.
pub fn parse_signature(sig: &[u8]) -> Option<(&[u8], &[u8])> {
    if sig.first() != Some(&0x30) {
        return None;
    }
    let len = *sig.get(1)? as usize;
    if len >= 0x80 || sig.len() != 2 + len {
        return None;
    }
    let body = &sig[2..];
    let (r, rest) = der_int(body)?;
    let (s, rest) = der_int(rest)?;
    rest.is_empty().then_some((r, s))
}

/// Verify a DER signature over `digest` (any length: it is truncated to the curve's
/// size, as ECDSA specifies) with an uncompressed SEC1 public key.
pub fn verify(curve: Curve, point: &[u8], digest: &[u8], sig: &[u8]) -> bool {
    let prm = match curve {
        Curve::P256 => P256,
        Curve::P384 => P384,
    };
    let size = prm.size;
    if point.len() != 1 + 2 * size || point[0] != 4 {
        return false;
    }
    let Some((r, s)) = parse_signature(sig) else { return false };
    let (Some(fp), Some(fnn)) = (Modulus::new(prm.p), Modulus::new(prm.n)) else { return false };
    // r, s in [1, n): anything else is not a signature (and r + n would otherwise pass).
    let (Some(r_n), Some(s_n)) = (fnn.below(r), fnn.below(s)) else { return false };
    if is_zero(&r_n) || is_zero(&s_n) {
        return false;
    }
    // The field and the curve constants.
    let three = fp.to_mont(&fp.element(&[3]).unwrap_or_default());
    let field = Field { f: fp, three };
    let to_mont_bytes = |b: &[u8]| field.f.element(b).map(|e| field.f.to_mont(&e));
    let (Some(qx), Some(qy)) = (to_mont_bytes(&point[1..1 + size]), to_mont_bytes(&point[1 + size..])) else { return false };
    // On the curve: y² = x³ − 3x + b.
    let (Some(b_m), Some(gx), Some(gy)) = (to_mont_bytes(prm.b), to_mont_bytes(prm.gx), to_mont_bytes(prm.gy)) else { return false };
    let lhs = field.sqr(&qy);
    let rhs = field.add(&field.mul(&field.sub(&field.sqr(&qx), &field.three), &qx), &b_m);
    if lhs != rhs {
        return false;
    }
    let one = field.f.one_mont();
    let q = Point { x: qx, y: qy, z: one.clone() };
    let g = Point { x: gx, y: gy, z: one };
    // e = the leftmost `size` bytes of the digest, reduced mod n.
    let e_bytes = &digest[..digest.len().min(size)];
    let Some(e) = fnn.element(e_bytes) else { return false };
    let w = fnn.pow_mont(&fnn.to_mont(&s_n), &fnn.minus_two_be());
    let u1 = to_be(&fnn.from_mont(&fnn.mont_mul(&fnn.to_mont(&e), &w)));
    let u2 = to_be(&fnn.from_mont(&fnn.mont_mul(&fnn.to_mont(&r_n), &w)));
    let sum = field.add_points(&field.mul_point(&u1, &g), &field.mul_point(&u2, &q));
    let Some(x) = field.affine_x(&sum) else { return false };
    // x mod n == r.
    let Some(x_n) = fnn.element(&x) else { return false };
    x_n == r_n
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_of(der: &[u8]) -> Vec<u8> {
        match super::super::x509::Cert::parse(der).unwrap().key().unwrap() {
            super::super::x509::Key::P256(p) | super::super::x509::Key::P384(p) => p,
            _ => panic!("not an EC key"),
        }
    }

    #[test]
    fn p256_tls_signature() {
        use sha2::Digest;
        let key = key_of(include_bytes!("../../testdata/leaf-p256.der"));
        let msg = include_bytes!("../../testdata/tls-msg.bin");
        let sig = include_bytes!("../../testdata/tls-sig-p256-sha256.bin");
        let d = sha2::Sha256::digest(msg);
        assert!(verify(Curve::P256, &key, &d, sig));
        let d2 = sha2::Sha256::digest(b"other");
        assert!(!verify(Curve::P256, &key, &d2, sig));
        let mut bad = sig.to_vec();
        bad[8] ^= 1;
        assert!(!verify(Curve::P256, &key, &d, &bad));
        let mut off_curve = key.clone();
        off_curve[5] ^= 1;
        assert!(!verify(Curve::P256, &off_curve, &d, sig));
    }

    #[test]
    fn p384_certificate_signature() {
        use sha2::Digest;
        let root = key_of(include_bytes!("../../testdata/ca-p384.der"));
        let leaf = super::super::x509::Cert::parse(include_bytes!("../../testdata/leaf-p384-chain.der")).unwrap();
        let d = sha2::Sha384::digest(leaf.tbs);
        assert!(verify(Curve::P384, &root, &d, leaf.signature));
        assert!(!verify(Curve::P384, &root, &sha2::Sha384::digest(b"x"), leaf.signature));
        assert!(!verify(Curve::P256, &root, &d, leaf.signature));
    }

    #[test]
    fn signature_parsing() {
        assert_eq!(parse_signature(&[0x30, 0x06, 0x02, 0x01, 0x05, 0x02, 0x01, 0x07]), Some((&[5u8][..], &[7u8][..])));
        assert_eq!(parse_signature(&[0x30, 0x03, 0x02, 0x01, 0x05]), None);
        assert_eq!(parse_signature(&[0x31, 0x00]), None);
        // Strict DER: no trailing bytes, no negative or padded integers, no empty ones.
        assert_eq!(parse_signature(&[0x30, 0x06, 0x02, 0x01, 0x05, 0x02, 0x01, 0x07, 0x00]), None);
        assert_eq!(parse_signature(&[0x30, 0x06, 0x02, 0x01, 0x85, 0x02, 0x01, 0x07]), None);
        assert_eq!(parse_signature(&[0x30, 0x07, 0x02, 0x02, 0x00, 0x05, 0x02, 0x01, 0x07]), None);
        assert_eq!(parse_signature(&[0x30, 0x07, 0x02, 0x02, 0x00, 0x85, 0x02, 0x01, 0x07]), Some((&[0u8, 0x85][..], &[7u8][..])));
        assert_eq!(parse_signature(&[0x30, 0x05, 0x02, 0x00, 0x02, 0x01, 0x07]), None);
    }

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    /// DER `ECDSA-Sig-Value` from r and s (minimal integers).
    fn der_sig(r: &[u8], s: &[u8]) -> Vec<u8> {
        fn int(v: &[u8]) -> Vec<u8> {
            let v = super::super::bignum::strip(v);
            let pad = v[0] & 0x80 != 0;
            let mut out = alloc::vec![0x02, (v.len() + pad as usize) as u8];
            if pad {
                out.push(0);
            }
            out.extend_from_slice(v);
            out
        }
        let (r, s) = (int(r), int(s));
        let mut out = alloc::vec![0x30, (r.len() + s.len()) as u8];
        out.extend_from_slice(&r);
        out.extend_from_slice(&s);
        out
    }

    /// Big-endian a + b (same length), one byte longer.
    fn add_be(a: &[u8], b: &[u8]) -> Vec<u8> {
        let mut out = alloc::vec![0u8; a.len() + 1];
        let mut carry = 0u16;
        for i in (0..a.len()).rev() {
            let v = a[i] as u16 + b[i] as u16 + carry;
            out[i + 1] = v as u8;
            carry = v >> 8;
        }
        out[0] = carry as u8;
        out
    }

    // RFC 6979 A.2.5 (P-256) and A.2.6 (P-384) known answers, each cross-checked with
    // OpenSSL. Digests longer than the curve are truncated, shorter ones are not.
    const P256_UX: &str = "60FED4BA255A9D31C961EB74C6356D68C049B8923B61FA6CE669622E60F29FB6";
    const P256_UY: &str = "7903FE1008B8BC99A41AE9E95628BC64F2F1B20C2D7E9F5177A3C294D4462299";
    const P384_UX: &str = "EC3A4E415B4E19A4568618029F427FA5DA9A8BC4AE92E02E06AAE5286B300C64DEF8F0EA9055866064A254515480BC13";
    const P384_UY: &str = "8015D9B72D7D57244EA8EF9AC0C621896708A59367F9DFB9F54CA84B3F1C9DB1288B231C3AE0D4FE7344FD2533264720";
    const P256_SAMPLE_SHA256: (&str, &str) = (
        "EFD48B2AACB6A8FD1140DD9CD45E81D69D2C877B56AAF991C34D0EA84EAF3716",
        "F7CB1C942D657C41D436C7A1B6E29F65F3E900DBB9AFF4064DC4AB2F843ACDA8",
    );
    const P256_SAMPLE_SHA384: (&str, &str) = (
        "0EAFEA039B20E9B42309FB1D89E213057CBF973DC0CFC8F129EDDDC800EF7719",
        "4861F0491E6998B9455193E34E7B0D284DDD7149A74B95B9261F13ABDE940954",
    );
    const P256_SAMPLE_SHA512: (&str, &str) = (
        "8496A60B5E9B47C825488827E0495B0E3FA109EC4568FD3F8D1097678EB97F00",
        "2362AB1ADBE2B8ADF9CB9EDAB740EA6049C028114F2460F96554F61FAE3302FE",
    );
    const P384_SAMPLE_SHA384: (&str, &str) = (
        "94EDBB92A5ECB8AAD4736E56C691916B3F88140666CE9FA73D64C4EA95AD133C81A648152E44ACF96E36DD1E80FABE46",
        "99EF4AEB15F178CEA1FE40DB2603138F130E740A19624526203B6351D0A3A94FA329C145786E679E7B82C71A38628AC8",
    );
    const P384_SAMPLE_SHA256: (&str, &str) = (
        "21B13D1E013C7FA1392D03C5F99AF8B30C570C6F98D4EA8E354B63A21D3DAA33BDE1E888E63355D92FA2B3C36D8FB2CD",
        "F3AA443FB107745BF4BD77CB3891674632068A10CA67E3D45DB2266FA7D1FEEBEFDC63ECCD1AC42EC0CB8668A4FA0AB0",
    );

    fn point(x: &str, y: &str) -> Vec<u8> {
        let mut p = alloc::vec![4u8];
        p.extend(hex(x));
        p.extend(hex(y));
        p
    }

    #[test]
    fn rfc6979_known_answers() {
        use sha2::Digest;
        let q256 = point(P256_UX, P256_UY);
        let q384 = point(P384_UX, P384_UY);
        let sig = |v: (&str, &str)| der_sig(&hex(v.0), &hex(v.1));
        assert!(verify(Curve::P256, &q256, &sha2::Sha256::digest(b"sample"), &sig(P256_SAMPLE_SHA256)));
        assert!(verify(Curve::P256, &q256, &sha2::Sha384::digest(b"sample"), &sig(P256_SAMPLE_SHA384)));
        assert!(verify(Curve::P256, &q256, &sha2::Sha512::digest(b"sample"), &sig(P256_SAMPLE_SHA512)));
        assert!(verify(Curve::P384, &q384, &sha2::Sha384::digest(b"sample"), &sig(P384_SAMPLE_SHA384)));
        assert!(verify(Curve::P384, &q384, &sha2::Sha256::digest(b"sample"), &sig(P384_SAMPLE_SHA256)));
        // Wrong message, wrong hash, wrong curve, wrong key.
        assert!(!verify(Curve::P256, &q256, &sha2::Sha256::digest(b"test"), &sig(P256_SAMPLE_SHA256)));
        assert!(!verify(Curve::P256, &q256, &sha2::Sha384::digest(b"sample"), &sig(P256_SAMPLE_SHA256)));
        assert!(!verify(Curve::P384, &q256, &sha2::Sha256::digest(b"sample"), &sig(P256_SAMPLE_SHA256)));
        assert!(!verify(Curve::P256, &q384, &sha2::Sha256::digest(b"sample"), &sig(P256_SAMPLE_SHA256)));
        assert!(!verify(Curve::P384, &q384, &sha2::Sha384::digest(b"sample"), &sig(P256_SAMPLE_SHA256)));
    }

    #[test]
    fn rejects_out_of_range_scalars_and_points() {
        use sha2::Digest;
        let q = point(P256_UX, P256_UY);
        let d = sha2::Sha256::digest(b"sample");
        let (r, s) = (hex(P256_SAMPLE_SHA256.0), hex(P256_SAMPLE_SHA256.1));
        assert!(verify(Curve::P256, &q, &d, &der_sig(&r, &s)));
        // r + n and s + n are congruent to r and s but are not valid signatures.
        assert!(!verify(Curve::P256, &q, &d, &der_sig(&add_be(&r, P256.n), &s)));
        assert!(!verify(Curve::P256, &q, &d, &der_sig(&r, &add_be(&s, P256.n))));
        // r or s zero, or equal to n.
        assert!(!verify(Curve::P256, &q, &d, &der_sig(&[0], &s)));
        assert!(!verify(Curve::P256, &q, &d, &der_sig(&r, &[0])));
        assert!(!verify(Curve::P256, &q, &d, &der_sig(P256.n, &s)));
        assert!(!verify(Curve::P256, &q, &d, &der_sig(&r, P256.n)));
        // A padded (non-minimal) r is not DER.
        let mut sig = der_sig(&r, &s);
        sig[1] += 1;
        sig[3] += 1;
        sig.insert(4, 0);
        assert!(!verify(Curve::P256, &q, &d, &sig));
        // Points off the curve, with x >= p, compressed, or of the wrong length.
        let mut off = q.clone();
        off[40] ^= 1;
        assert!(!verify(Curve::P256, &off, &d, &der_sig(&r, &s)));
        let mut big_x = q.clone();
        big_x[1..33].copy_from_slice(P256.p);
        assert!(!verify(Curve::P256, &big_x, &d, &der_sig(&r, &s)));
        let mut compressed = q.clone();
        compressed[0] = 2;
        assert!(!verify(Curve::P256, &compressed, &d, &der_sig(&r, &s)));
        assert!(!verify(Curve::P256, &q[..64], &d, &der_sig(&r, &s)));
        assert!(!verify(Curve::P256, &[0u8; 65], &d, &der_sig(&r, &s)));
        // P-384's r and s must be below its n too.
        let q384 = point(P384_UX, P384_UY);
        let d384 = sha2::Sha384::digest(b"sample");
        let (r, s) = (hex(P384_SAMPLE_SHA384.0), hex(P384_SAMPLE_SHA384.1));
        assert!(verify(Curve::P384, &q384, &d384, &der_sig(&r, &s)));
        assert!(!verify(Curve::P384, &q384, &d384, &der_sig(&add_be(&r, P384.n), &s)));
    }
}
