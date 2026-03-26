use serde::{Deserialize, Serialize};

use crate::{GameState, SessionId};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub version: u32,
    pub session_id: SessionId,
    pub state: GameState,
}

impl Snapshot {
    pub fn new(session_id: SessionId, state: GameState) -> Self {
        Self {
            version: 1,
            session_id,
            state,
        }
    }
}
