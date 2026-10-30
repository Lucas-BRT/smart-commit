//! Python's `random.Random(seed).randbytes(n)`, for the binary files of the
//! scenarios: the same seed gives the same bytes as the Python tool made.
//! MT19937, seeded the way CPython seeds from an int (`init_by_array`).

const N: usize = 624;
const M: usize = 397;

pub struct Mt {
    state: [u32; N],
    index: usize,
}

impl Mt {
    fn init_genrand(seed: u32) -> [u32; N] {
        let mut state = [0u32; N];
        state[0] = seed;
        for i in 1..N {
            state[i] = 1_812_433_253u32
                .wrapping_mul(state[i - 1] ^ (state[i - 1] >> 30))
                .wrapping_add(i as u32);
        }
        state
    }

    /// `random.Random(seed)` for a non-negative int seed.
    pub fn new(seed: u64) -> Self {
        let mut key = vec![seed as u32];
        if seed >> 32 != 0 {
            key.push((seed >> 32) as u32);
        }
        let mut state = Self::init_genrand(19_650_218);
        let (mut i, mut j) = (1usize, 0usize);
        for _ in 0..N.max(key.len()) {
            state[i] = (state[i] ^ (state[i - 1] ^ (state[i - 1] >> 30)).wrapping_mul(1_664_525))
                .wrapping_add(key[j])
                .wrapping_add(j as u32);
            i += 1;
            j += 1;
            if i >= N {
                state[0] = state[N - 1];
                i = 1;
            }
            if j >= key.len() {
                j = 0;
            }
        }
        for _ in 0..N - 1 {
            state[i] = (state[i]
                ^ (state[i - 1] ^ (state[i - 1] >> 30)).wrapping_mul(1_566_083_941))
            .wrapping_sub(i as u32);
            i += 1;
            if i >= N {
                state[0] = state[N - 1];
                i = 1;
            }
        }
        state[0] = 0x8000_0000;
        Self { state, index: N }
    }

    fn next_u32(&mut self) -> u32 {
        if self.index >= N {
            for k in 0..N {
                let y = (self.state[k] & 0x8000_0000) | (self.state[(k + 1) % N] & 0x7fff_ffff);
                let mut next = self.state[(k + M) % N] ^ (y >> 1);
                if y & 1 != 0 {
                    next ^= 0x9908_b0df;
                }
                self.state[k] = next;
            }
            self.index = 0;
        }
        let mut y = self.state[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^ (y >> 18)
    }

    /// `randbytes(n)`: `getrandbits(8 * n)` as little-endian bytes, whose
    /// last word keeps its high bits.
    pub fn randbytes(&mut self, n: usize) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(n);
        let mut left = n;
        while left > 0 {
            let mut word = self.next_u32();
            let take = left.min(4);
            if take < 4 {
                word >>= 32 - 8 * take as u32;
            }
            bytes.extend_from_slice(&word.to_le_bytes()[..take]);
            left -= take;
        }
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // From CPython 3.12: random.Random(seed).randbytes(n).hex().
    #[test]
    fn the_bytes_are_cpython_s() {
        assert_eq!(hex(&Mt::new(0).randbytes(8)), "cd072cd8be6f9f62");
        assert_eq!(hex(&Mt::new(1).randbytes(6)), "f5b16522b791");
        assert_eq!(
            hex(&Mt::new(5).randbytes(20)),
            "457c769f39d8644199c0e5bdbcfbc85b37ce91cb"
        );
        // A seed of more than 32 bits, and a length that is not a multiple of 4.
        assert_eq!(hex(&Mt::new((1 << 33) + 3).randbytes(5)), "fd3feb3c97");
    }

    #[test]
    fn one_seed_gives_one_stream() {
        assert_eq!(Mt::new(7).randbytes(100), Mt::new(7).randbytes(100));
        assert_ne!(Mt::new(7).randbytes(100), Mt::new(8).randbytes(100));
        assert_eq!(Mt::new(7).randbytes(3).len(), 3);
    }

    use smart_commit::models::hex;
}
