//! A minimal modular-arithmetic kernel for signature verification: Montgomery
//! multiplication over one odd modulus, with the few extras RSA and ECDSA need
//! (addition, subtraction, exponentiation, conversion). Numbers are little-endian `u32`
//! limbs of the modulus' length; nothing here handles secrets, so nothing is
//! constant-time, and every operation allocates its result (one request at a time).

use alloc::vec;
use alloc::vec::Vec;

/// Limbs of one number (little-endian).
pub type Limbs = Vec<u32>;

/// Strip leading zero bytes.
pub fn strip(b: &[u8]) -> &[u8] {
    let mut i = 0;
    while i + 1 < b.len() && b[i] == 0 {
        i += 1;
    }
    &b[i..]
}

/// Big-endian bytes to `k` limbs; `None` when they do not fit.
pub fn from_be(bytes: &[u8], k: usize) -> Option<Limbs> {
    let bytes = strip(bytes);
    if bytes.len() > k * 4 {
        return None;
    }
    let mut out = vec![0u32; k];
    for (i, b) in bytes.iter().rev().enumerate() {
        out[i / 4] |= (*b as u32) << (8 * (i % 4));
    }
    Some(out)
}

/// Limbs to big-endian bytes (`4k` of them).
pub fn to_be(limbs: &[u32]) -> Vec<u8> {
    let k = limbs.len();
    let mut bytes = vec![0u8; k * 4];
    for (i, limb) in limbs.iter().enumerate() {
        bytes[k * 4 - 4 * i - 4..k * 4 - 4 * i].copy_from_slice(&limb.to_be_bytes());
    }
    bytes
}

/// a >= b (same length).
pub fn ge(a: &[u32], b: &[u32]) -> bool {
    for i in (0..a.len()).rev() {
        if a[i] != b[i] {
            return a[i] > b[i];
        }
    }
    true
}

/// Whether every limb is zero.
pub fn is_zero(a: &[u32]) -> bool {
    a.iter().all(|l| *l == 0)
}

/// a -= b (a >= b).
fn sub_in_place(a: &mut [u32], b: &[u32]) {
    let mut borrow = 0u64;
    for i in 0..a.len() {
        let d = (a[i] as u64).wrapping_sub(b[i] as u64).wrapping_sub(borrow);
        a[i] = d as u32;
        borrow = (d >> 63) & 1;
    }
}

/// An odd modulus with its Montgomery constants.
pub struct Modulus {
    n: Limbs,
    /// -n^-1 mod 2^32.
    n0: u32,
    /// R^2 mod n, with R = 2^(32k).
    r2: Limbs,
}

impl Modulus {
    /// For an odd modulus of 8 to 1024 bytes.
    pub fn new(n_bytes: &[u8]) -> Option<Modulus> {
        let n_bytes = strip(n_bytes);
        if n_bytes.is_empty() || n_bytes[0] == 0 || n_bytes[n_bytes.len() - 1] & 1 == 0 || n_bytes.len() < 8 || n_bytes.len() > 1024 {
            return None;
        }
        let k = n_bytes.len().div_ceil(4);
        let n = from_be(n_bytes, k)?;
        let mut inv: u32 = 1;
        for _ in 0..5 {
            inv = inv.wrapping_mul(2u32.wrapping_sub(n[0].wrapping_mul(inv)));
        }
        let n0 = inv.wrapping_neg();
        // r2 = 2^(64k) mod n by doubling 1 with conditional subtraction.
        let mut r2 = vec![0u32; k];
        r2[0] = 1;
        for _ in 0..(64 * k) {
            let mut carry = 0u32;
            for limb in r2.iter_mut() {
                let v = ((*limb as u64) << 1) | carry as u64;
                *limb = v as u32;
                carry = (v >> 32) as u32;
            }
            if carry != 0 || ge(&r2, &n) {
                sub_in_place(&mut r2, &n);
            }
        }
        Some(Modulus { n, n0, r2 })
    }

    /// Limbs per number.
    pub fn limbs(&self) -> usize {
        self.n.len()
    }

    /// The modulus' length in bits.
    pub fn bit_len(&self) -> usize {
        let mut bits = self.n.len() * 32;
        for limb in self.n.iter().rev() {
            if *limb == 0 {
                bits -= 32;
            } else {
                bits -= limb.leading_zeros() as usize;
                break;
            }
        }
        bits
    }

    /// The modulus' length in bytes.
    pub fn byte_len(&self) -> usize {
        self.bit_len().div_ceil(8)
    }

    /// Big-endian bytes as limbs below the modulus (one subtraction when just over,
    /// which covers ECDSA's hash reduction).
    pub fn element(&self, bytes: &[u8]) -> Option<Limbs> {
        let mut x = from_be(bytes, self.n.len())?;
        if ge(&x, &self.n) {
            sub_in_place(&mut x, &self.n);
            if ge(&x, &self.n) {
                return None;
            }
        }
        Some(x)
    }

    /// Montgomery product a·b·R^-1 mod n (CIOS).
    pub fn mont_mul(&self, a: &[u32], b: &[u32]) -> Limbs {
        let k = self.n.len();
        let mut t = vec![0u32; k + 2];
        for &bi in b.iter().take(k) {
            let mut carry = 0u64;
            for j in 0..k {
                let v = t[j] as u64 + (a[j] as u64) * (bi as u64) + carry;
                t[j] = v as u32;
                carry = v >> 32;
            }
            let v = t[k] as u64 + carry;
            t[k] = v as u32;
            t[k + 1] = (v >> 32) as u32;
            let m = t[0].wrapping_mul(self.n0);
            let mut carry = ((t[0] as u64) + (m as u64) * (self.n[0] as u64)) >> 32;
            for j in 1..k {
                let v = t[j] as u64 + (m as u64) * (self.n[j] as u64) + carry;
                t[j - 1] = v as u32;
                carry = v >> 32;
            }
            let v = t[k] as u64 + carry;
            t[k - 1] = v as u32;
            t[k] = t[k + 1] + (v >> 32) as u32;
            t[k + 1] = 0;
        }
        if t[k] != 0 || ge(&t[..k], &self.n) {
            sub_in_place(&mut t[..k], &self.n);
        }
        t.truncate(k);
        t
    }

    /// Into Montgomery form.
    pub fn to_mont(&self, a: &[u32]) -> Limbs {
        self.mont_mul(a, &self.r2)
    }

    /// Out of Montgomery form.
    pub fn from_mont(&self, a: &[u32]) -> Limbs {
        let mut one = vec![0u32; self.n.len()];
        one[0] = 1;
        self.mont_mul(a, &one)
    }

    /// 1 in Montgomery form.
    pub fn one_mont(&self) -> Limbs {
        let mut one = vec![0u32; self.n.len()];
        one[0] = 1;
        self.to_mont(&one)
    }

    /// a + b mod n (inputs below n).
    pub fn add(&self, a: &[u32], b: &[u32]) -> Limbs {
        let k = self.n.len();
        let mut out = vec![0u32; k];
        let mut carry = 0u64;
        for i in 0..k {
            let v = a[i] as u64 + b[i] as u64 + carry;
            out[i] = v as u32;
            carry = v >> 32;
        }
        if carry != 0 || ge(&out, &self.n) {
            sub_in_place(&mut out, &self.n);
        }
        out
    }

    /// a - b mod n (inputs below n).
    pub fn sub(&self, a: &[u32], b: &[u32]) -> Limbs {
        let mut out = a.to_vec();
        if ge(a, b) {
            sub_in_place(&mut out, b);
        } else {
            // a - b + n
            let mut carry = 0u64;
            for (o, n) in out.iter_mut().zip(self.n.iter()) {
                let v = *o as u64 + *n as u64 + carry;
                *o = v as u32;
                carry = v >> 32;
            }
            sub_in_place(&mut out, b);
        }
        out
    }

    /// base^exp mod n for `base` in Montgomery form and a big-endian exponent; the
    /// result is in Montgomery form.
    pub fn pow_mont(&self, base: &[u32], exp: &[u8]) -> Limbs {
        let mut acc = self.one_mont();
        let mut started = false;
        for byte in strip(exp) {
            for bit in (0..8).rev() {
                if started {
                    acc = self.mont_mul(&acc, &acc);
                }
                if (byte >> bit) & 1 == 1 {
                    acc = if started { self.mont_mul(&acc, base) } else { base.to_vec() };
                    started = true;
                }
            }
        }
        acc
    }

    /// The modulus minus two, as big-endian bytes (the exponent for a Fermat inverse).
    pub fn minus_two_be(&self) -> Vec<u8> {
        let mut m = self.n.clone();
        let two = {
            let mut t = vec![0u32; m.len()];
            t[0] = 2;
            t
        };
        sub_in_place(&mut m, &two);
        to_be(&m)
    }

    /// s^e mod n as big-endian bytes of the modulus length, for a small public
    /// exponent (RSA); `None` when `s` is not below the modulus.
    pub fn pow_small(&self, s: &[u8], e: &[u8]) -> Option<Vec<u8>> {
        let s = from_be(s, self.n.len())?;
        if ge(&s, &self.n) {
            return None;
        }
        let e = strip(e);
        if e.is_empty() || e.len() > 4 {
            return None;
        }
        let base = self.to_mont(&s);
        let out = self.from_mont(&self.pow_mont(&base, e));
        Some(to_be(&out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn montgomery_matches_small_case() {
        let mut n = vec![0u8; 64];
        n[56..].copy_from_slice(&0xffffffffffffffc5u64.to_be_bytes()); // 2^64 - 59, prime
        let m = Modulus::new(&n).unwrap();
        let out = m.pow_small(&[3], &[1, 0, 1]).unwrap();
        let got = u64::from_be_bytes(out[out.len() - 8..].try_into().unwrap());
        let modulus = 0xffffffffffffffc5u128;
        let mut acc: u128 = 1;
        for _ in 0..65537 {
            acc = acc * 3 % modulus;
        }
        assert_eq!(got as u128, acc);
        // Fermat inverse: 3 * 3^(p-2) == 1.
        let three = m.to_mont(&from_be(&[3], m.limbs()).unwrap());
        let inv = m.pow_mont(&three, &m.minus_two_be());
        let one = m.from_mont(&m.mont_mul(&three, &inv));
        assert_eq!(one[0], 1);
        assert!(one[1..].iter().all(|l| *l == 0));
        // add / sub wrap correctly.
        let a = from_be(&[5], m.limbs()).unwrap();
        let b = from_be(&[7], m.limbs()).unwrap();
        let d = m.sub(&a, &b);
        assert_eq!(m.add(&d, &b), a);
    }
}
