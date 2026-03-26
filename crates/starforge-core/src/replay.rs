use serde::{Deserialize, Serialize};

use crate::{CommandEnvelope, TickId};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayLog {
    pub accepted_commands: Vec<CommandEnvelope>,
}

impl ReplayLog {
    pub fn max_apply_tick(&self) -> TickId {
        self.accepted_commands
            .iter()
            .map(|command| command.apply_at_tick)
            .max()
            .unwrap_or_default()
    }
}
