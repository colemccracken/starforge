use serde::{Deserialize, Serialize};

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct MatchSeed(pub u64);

impl MatchSeed {
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct SessionId(pub u64);

impl SessionId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct TickId(pub u64);

impl TickId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct PlayerId(pub u8);

impl PlayerId {
    pub const fn new(value: u8) -> Self {
        Self(value)
    }
}
