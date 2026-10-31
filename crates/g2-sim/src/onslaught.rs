//! Onslaught: an opponent with no base and no economy. Its waves appear in front of its start
//! on a fixed schedule, larger each time, and attack. The player wins by outlasting the last
//! wave, and loses the usual way, when their base is gone.
//!
//! The archive barely describes it: the mode was due in the alpha update of the week of
//! 28 October 2013, which never shipped. Patch preview #4 (24 October 2013) planned two
//! players with a prebuilt base and no resource collection against eight waves of an AI
//! general, and a 2013 alpha recording counts down 1:58 to its first wave and about 1:45
//! between the next. Here the defender plays a whole skirmish economy, so the schedule, the
//! wave sizes and the unit mix are tuned by measurement instead. The schedule is a function of
//! the tick alone, so it adds no state to the hash.

use crate::world::TICKS_PER_SECOND;

/// The first wave appears at 2:30.
pub const FIRST_WAVE: u32 = 150 * TICKS_PER_SECOND;
/// Then one every 75 seconds.
pub const WAVE_EVERY: u32 = 75 * TICKS_PER_SECOND;
pub const WAVES: u32 = 10;

/// Units in wave `k` (0-based), five more every three waves: 2, 3, 5, 7, 8, 10, 12, 13, 15, 17.
/// One more each wave (2 to 11) stopped being a threat once the Normal AI's factories and
/// collectors were fixed: it held 716 of 768 decided matches on all 86 skirmish maps.
pub fn wave_size(k: u32) -> u32 {
    2 + 5 * k / 3
}

/// Experience every unit of wave `k` arrives with: none for the first three, then enough for
/// the first rank, the second from wave 7 and the third for the last two. A defender's army
/// gains ranks fighting the waves, so waves of green units soon stop being a threat.
pub fn wave_experience(k: u32) -> u32 {
    match k {
        0..=2 => 0,
        3..=5 => 3,
        6..=7 => 6,
        _ => 10,
    }
}

/// The tick wave `k` appears on.
pub fn wave_tick(k: u32) -> u32 {
    FIRST_WAVE + k * WAVE_EVERY
}

/// The wave that appears on `tick`, if one does.
pub fn wave_at(tick: u32) -> Option<u32> {
    let k = tick.checked_sub(FIRST_WAVE)? / WAVE_EVERY;
    (k < WAVES && wave_tick(k) == tick).then_some(k)
}

/// The next wave to appear at or after `tick`, and when; `None` once the last one is out.
pub fn next_wave(tick: u32) -> Option<(u32, u32)> {
    let k = tick.saturating_sub(FIRST_WAVE).div_ceil(WAVE_EVERY);
    let k = if tick < FIRST_WAVE { 0 } else { k };
    (k < WAVES).then(|| (k, wave_tick(k)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_schedule_is_consistent() {
        let ticks: Vec<u32> = (0..wave_tick(WAVES) + 10).filter(|&t| wave_at(t).is_some()).collect();
        assert_eq!(ticks, (0..WAVES).map(wave_tick).collect::<Vec<_>>());
        assert_eq!(next_wave(0), Some((0, FIRST_WAVE)));
        assert_eq!(next_wave(FIRST_WAVE), Some((0, FIRST_WAVE)));
        assert_eq!(next_wave(FIRST_WAVE + 1), Some((1, wave_tick(1))));
        assert_eq!(next_wave(wave_tick(WAVES - 1)), Some((WAVES - 1, wave_tick(WAVES - 1))));
        assert_eq!(next_wave(wave_tick(WAVES - 1) + 1), None);
    }
}
