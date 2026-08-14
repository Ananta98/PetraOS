//! Process and thread nice values and weighting for the fair scheduler.
//!
//! Nice values range from `-20` (highest priority / maximum CPU weight) to `19`
//! (lowest priority / minimum CPU weight), with a default of `0`.

/// Minimum valid nice value (highest priority).
pub const MIN_NICE: i8 = -20;

/// Maximum valid nice value (lowest priority).
pub const MAX_NICE: i8 = 19;

/// The base weight corresponding to `nice = 0`.
pub const NICE_0_WEIGHT: u32 = 1024;

/// Standard weight lookup table for nice values from -20 to 19 (40 levels).
///
/// Each step corresponds to approximately a ~1.25x (10-25%) difference in CPU allocation weight,
/// matching standard proportional-share scheduling scales.
pub const NICE_TO_WEIGHT: [u32; 40] = [
    /* -20 */ 88761, 71755, 56483, 46273, 36291,
    /* -15 */ 29154, 23254, 18705, 14949, 11916,
    /* -10 */ 9548, 7620, 6100, 4904, 3906,
    /*  -5 */ 3121, 2501, 1991, 1586, 1277,
    /*   0 */ 1024, 820, 655, 526, 423,
    /*   5 */ 335, 272, 215, 172, 137,
    /*  10 */ 110, 87, 70, 56, 45,
    /*  15 */ 36, 29, 23, 18, 15,
];

/// Converts a nice value into its corresponding CPU weight.
pub const fn nice_to_weight(nice: Nice) -> u32 {
    let index = (nice.value() - MIN_NICE) as usize;
    NICE_TO_WEIGHT[index]
}

/// Represents a validated thread nice value constrained to `[-20, 19]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Nice(i8);

impl Nice {
    /// Minimum nice value (`-20`, highest scheduling priority).
    pub const MIN: Self = Self(MIN_NICE);

    /// Maximum nice value (`19`, lowest scheduling priority).
    pub const MAX: Self = Self(MAX_NICE);

    /// Default nice value (`0`, standard priority).
    pub const DEFAULT: Self = Self(0);

    /// Creates a new `Nice` value, returning an error if out of bounds.
    pub const fn new(val: i8) -> Result<Self, &'static str> {
        if val >= MIN_NICE && val <= MAX_NICE {
            Ok(Self(val))
        } else {
            Err("Nice value must be between -20 and 19")
        }
    }

    /// Creates a `Nice` value by clamping the input into the valid `[-20, 19]` range.
    pub const fn from_raw_clamped(val: i8) -> Self {
        if val < MIN_NICE {
            Self(MIN_NICE)
        } else if val > MAX_NICE {
            Self(MAX_NICE)
        } else {
            Self(val)
        }
    }

    /// Returns the underlying `i8` nice value.
    pub const fn value(&self) -> i8 {
        self.0
    }

    /// Returns the CPU weight associated with this nice value.
    pub const fn weight(&self) -> u32 {
        nice_to_weight(*self)
    }
}

impl Default for Nice {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl From<Nice> for i8 {
    fn from(nice: Nice) -> Self {
        nice.0
    }
}

impl TryFrom<i8> for Nice {
    type Error = &'static str;

    fn try_from(val: i8) -> Result<Self, Self::Error> {
        Self::new(val)
    }
}
