use core::ops::RangeInclusive;

pub const NICE_RANGE: RangeInclusive<i32> = -20..=19;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NiceWeight(i32);

impl NiceWeight {
    pub fn new(nice: i32) -> Self {
        Self(nice.clamp(*NICE_RANGE.start(), *NICE_RANGE.end()))
    }

    pub fn value(&self) -> i32 {
        self.0
    }

    /// Calculate standard CFS weight: approx 1024 / (1.25 ^ nice)
    pub fn to_weight(&self) -> u64 {
        let mut weight: u64 = 1024;
        let nice = self.0;
        
        if nice < 0 {
            // weight = 1024 * (5/4) ^ (-nice)
            for _ in 0..(-nice) {
                weight = weight * 5 / 4;
            }
        } else if nice > 0 {
            // weight = 1024 / (5/4) ^ nice
            for _ in 0..nice {
                weight = weight * 4 / 5;
            }
        }
        weight.max(15) // clamp bottom 
    }
}
