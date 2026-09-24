//! A small deterministic PRNG, so puzzle generation is reproducible from a
//! seed without pulling in a dependency.

/// SplitMix64.
pub struct Rng {
    state: u64,
}

const GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(GAMMA);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A value in `0..n`, using the high bits to avoid modulo bias.
    pub fn below(&mut self, n: usize) -> usize {
        debug_assert!(n > 0);
        ((self.next_u64() as u128 * n as u128) >> 64) as usize
    }

    /// Fisher-Yates.
    pub fn shuffle<T>(&mut self, slice: &mut [T]) {
        for i in (1..slice.len()).rev() {
            slice.swap(i, self.below(i + 1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_gives_same_sequence() {
        let a: Vec<u64> = (0..8).map(|_| Rng::new(7).next_u64()).collect();
        let mut rng = Rng::new(7);
        assert!(a.iter().all(|&x| x == a[0]));
        assert_ne!(rng.next_u64(), rng.next_u64());
    }

    #[test]
    fn below_stays_in_range_and_covers_it() {
        let mut rng = Rng::new(1);
        let mut seen = [false; 9];
        for _ in 0..1000 {
            let v = rng.below(9);
            assert!(v < 9);
            seen[v] = true;
        }
        assert!(seen.iter().all(|&s| s));
    }

    #[test]
    fn shuffle_is_a_permutation() {
        let mut rng = Rng::new(42);
        let mut items: Vec<usize> = (0..81).collect();
        rng.shuffle(&mut items);
        assert_ne!(items, (0..81).collect::<Vec<_>>());
        items.sort_unstable();
        assert_eq!(items, (0..81).collect::<Vec<_>>());
    }
}
