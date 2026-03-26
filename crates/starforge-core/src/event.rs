use serde::{Deserialize, Serialize};

use crate::{PlayerId, TickId};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventRecord {
    pub tick_id: TickId,
    pub player_id: Option<PlayerId>,
    pub kind: EventKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventKind {
    SessionCreated,
    TickAdvanced,
    CommandAccepted,
    CommandApplied,
    CommandRejected,
}
