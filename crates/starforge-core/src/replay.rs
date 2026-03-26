use serde::{Deserialize, Serialize};

use crate::CommandEnvelope;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayLog {
    pub accepted_commands: Vec<CommandEnvelope>,
}
