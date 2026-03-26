use starforge_core::{CommandEnvelope, PlayerId, SessionId, TickId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservationSnapshot {
    pub session_id: SessionId,
    pub player_id: PlayerId,
    pub tick_id: TickId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InferenceError {
    pub message: String,
}

pub trait InferenceRuntime {
    fn health_check(&self) -> Result<(), InferenceError>;
    fn generate_commands(
        &self,
        snapshot: &ObservationSnapshot,
    ) -> Result<Vec<CommandEnvelope>, InferenceError>;
}

#[derive(Clone, Debug, Default)]
pub struct NoopRuntime;

impl InferenceRuntime for NoopRuntime {
    fn health_check(&self) -> Result<(), InferenceError> {
        Ok(())
    }

    fn generate_commands(
        &self,
        _snapshot: &ObservationSnapshot,
    ) -> Result<Vec<CommandEnvelope>, InferenceError> {
        Ok(Vec::new())
    }
}
