/// PCG32 (XSH RR, 64-bit state), M.E. O'Neill's reference algorithm. Only the simulation
/// draws from it, so its state is part of the replay hash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pcg32 {
    state: u64,
    inc: u64,
}

impl Pcg32 {
    const MUL: u64 = 6364136223846793005;

    pub fn new(seed: u64, stream: u64) -> Pcg32 {
        let mut rng = Pcg32 { state: 0, inc: (stream << 1) | 1 };
        rng.next_u32();
        rng.state = rng.state.wrapping_add(seed);
        rng.next_u32();
        rng
    }

    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(Self::MUL).wrapping_add(self.inc);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// Uniform in `0..bound` without modulo bias. `bound` must be non-zero.
    pub fn below(&mut self, bound: u32) -> u32 {
        let threshold = bound.wrapping_neg() % bound;
        loop {
            let r = self.next_u32();
            if r >= threshold {
                return r % bound;
            }
        }
    }

    pub(crate) fn raw_state(&self) -> (u64, u64) {
        (self.state, self.inc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_pcg32_reference_vectors() {
        // pcg32-global demo: seed 42, stream 54.
        let mut rng = Pcg32::new(42, 54);
        let expected = [0xa15c02b7, 0x7b47f409, 0xba1d3330, 0x83d2f293, 0xbfa4784b, 0xcbed606e];
        for e in expected {
            assert_eq!(rng.next_u32(), e);
        }
    }

    #[test]
    fn below_stays_in_range() {
        let mut rng = Pcg32::new(7, 1);
        for bound in [1, 2, 3, 10, 1000, u32::MAX] {
            for _ in 0..200 {
                assert!(rng.below(bound) < bound);
            }
        }
    }
}
