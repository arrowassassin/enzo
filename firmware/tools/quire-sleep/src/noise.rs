//! Deterministic randomness: a seeded xorshift generator and lattice value noise.

/// A small, fast PRNG (xorshift64*). Seeded per image so output is reproducible.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    /// Seed the generator; zero is remapped so the state never sticks.
    pub fn new(seed: u64) -> Self {
        Rng(if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed })
    }
    /// Next 32 random bits.
    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 32) as u32
    }
    /// Uniform in `[0, 1)`.
    pub fn f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }
    /// Uniform in `[lo, hi)`.
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f32()
    }
    /// Uniform integer in `[lo, hi)`.
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            self.next_u32() % n
        }
    }
    /// `true` with probability `p`.
    pub fn chance(&mut self, p: f32) -> bool {
        self.f32() < p
    }
    /// A derived generator for a sub-task, so adding draws in one place does not
    /// reshuffle everything after it.
    pub fn fork(&mut self, salt: u64) -> Rng {
        Rng::new(self.0.rotate_left(17) ^ salt.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ self.next_u32() as u64)
    }
}

fn hash2(ix: i32, iy: i32, seed: u32) -> f32 {
    let mut h = (ix as u32).wrapping_mul(0x8da6_b343) ^ (iy as u32).wrapping_mul(0xd816_3841) ^ seed.wrapping_mul(0x9E37_79B9);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a_2d39);
    h ^= h >> 15;
    (h >> 8) as f32 / (1u32 << 24) as f32
}

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// Smooth value noise in `[0, 1]` at a 1-D coordinate.
pub fn noise1(x: f32, seed: u32) -> f32 {
    let ix = x.floor();
    let t = smooth(x - ix);
    let a = hash2(ix as i32, 0, seed);
    let b = hash2(ix as i32 + 1, 0, seed);
    a + (b - a) * t
}

/// Smooth value noise in `[0, 1]` at a 2-D coordinate.
pub fn noise2(x: f32, y: f32, seed: u32) -> f32 {
    let (ix, iy) = (x.floor(), y.floor());
    let (tx, ty) = (smooth(x - ix), smooth(y - iy));
    let (ix, iy) = (ix as i32, iy as i32);
    let a = hash2(ix, iy, seed);
    let b = hash2(ix + 1, iy, seed);
    let c = hash2(ix, iy + 1, seed);
    let d = hash2(ix + 1, iy + 1, seed);
    let top = a + (b - a) * tx;
    let bot = c + (d - c) * tx;
    top + (bot - top) * ty
}

/// Fractional Brownian motion over [`noise1`], normalised to about `[0, 1]`.
pub fn fbm1(x: f32, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut freq, mut norm) = (0.0, 1.0, 1.0, 0.0);
    for o in 0..octaves {
        sum += amp * noise1(x * freq, seed.wrapping_add(o * 131));
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / norm
}

/// Fractional Brownian motion over [`noise2`], normalised to about `[0, 1]`.
pub fn fbm2(x: f32, y: f32, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut freq, mut norm) = (0.0, 1.0, 1.0, 0.0);
    for o in 0..octaves {
        sum += amp * noise2(x * freq, y * freq, seed.wrapping_add(o * 131));
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / norm
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng_is_deterministic_and_uniform() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        assert_eq!(a.next_u32(), b.next_u32());
        let mean: f32 = (0..10_000).map(|_| a.f32()).sum::<f32>() / 10_000.0;
        assert!((mean - 0.5).abs() < 0.02, "{mean}");
    }

    #[test]
    fn noise_is_bounded() {
        for i in 0..500 {
            let v = fbm2(i as f32 * 0.37, i as f32 * 0.11, 5, 7);
            assert!((0.0..=1.0).contains(&v), "{v}");
        }
    }
}
