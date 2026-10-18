//! The C library's random numbers, as the original draws them: one
//! sequence for the whole game (MSVC's `rand`, 0x5c2ac1), seeded once as
//! the game starts, from the clock (0x499b40 → `srand`). It is not saved.

use std::sync::atomic::{AtomicU32, Ordering};

/// The C library's own seed until `srand` is called.
static STATE: AtomicU32 = AtomicU32::new(1);

/// Seeds the sequence, as `srand`.
pub fn srand(seed: u32) {
    STATE.store(seed, Ordering::Relaxed);
}

/// The next number, 0 to 32767, as `rand`.
pub fn rand() -> u32 {
    let next = STATE
        .load(Ordering::Relaxed)
        .wrapping_mul(0x343fd)
        .wrapping_add(0x269ec3);
    STATE.store(next, Ordering::Relaxed);
    (next >> 16) & 0x7fff
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_as_the_c_library_does() {
        srand(1);
        // MSVC's first numbers from its default seed.
        assert_eq!([rand(), rand(), rand()], [41, 18467, 6334]);
    }
}
