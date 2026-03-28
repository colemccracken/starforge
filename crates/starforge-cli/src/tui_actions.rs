use starforge_api::{PlayerFrameResponse, SessionMode, SessionPhase};
use starforge_core::{
    InfrastructureCondition, InfrastructureKind, InfrastructureProjectKind, LocationView,
    LocationVisibility, PlayerId, RelayStatus, ResearchBranch, ResourceStockpiles, TerritoryState,
    balance::{
        buildable_infrastructure_kinds, construction_preview, is_unique_infrastructure,
        repair_preview, research_preview, strategic_strike_cost, training_preview,
    },
};

use crate::{
    cli::{
        PlayerScopedCommand, ScopedBudgetArgs, ScopedInfrastructureArgs, ScopedResearchArgs,
        ScopedTrainArgs, TransitSpec,
    },
    render::{format_stockpiles, render_research_branch},
};

const COMMANDS_LOCKED_REASON: &str = "commands are disabled until the competitive match starts";
const SURVEY_REQUIRED_REASON: &str = "survey the destination before issuing this action";
const SANDBOX_SPEED_CHOICES_MS: [u64; 3] = [5_000, 2_500, 1_000];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaneFocus {
    Locations,
    Actions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ActionId {
    Survey,
    Pacify,
    Claim,
    Assault,
    Strike,
    Build,
    Repair,
    Relay,
    Status,
    Map,
    Events,
    Budget,
    Research,
    Training,
    Ready,
    PauseResume,
    Speed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActionGroup {
    Location,
    Session,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ActionAvailability {
    Enabled,
    Disabled { reason: String },
}

impl ActionAvailability {
    pub(crate) const fn is_enabled(&self) -> bool {
        matches!(self, Self::Enabled)
    }

    pub(crate) fn reason(&self) -> Option<&str> {
        match self {
            Self::Enabled => None,
            Self::Disabled { reason } => Some(reason.as_str()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActionItem {
    pub(crate) id: ActionId,
    pub(crate) group: ActionGroup,
    pub(crate) label: String,
    pub(crate) preview: String,
    pub(crate) availability: ActionAvailability,
    pub(crate) opens_form: bool,
}

impl ActionItem {
    pub(crate) fn summary(&self) -> String {
        match &self.availability {
            ActionAvailability::Enabled => self.preview.clone(),
            ActionAvailability::Disabled { reason } => {
                format!("Unavailable: {reason}")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActionChoice<T> {
    pub(crate) label: String,
    pub(crate) value: T,
    pub(crate) details: String,
    pub(crate) availability: ActionAvailability,
}

impl<T> ActionChoice<T> {
    pub(crate) const fn is_enabled(&self) -> bool {
        self.availability.is_enabled()
    }

    #[cfg(test)]
    pub(crate) fn summary(&self) -> String {
        choice_message(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EventHistoryChoice {
    AllVisible,
    Last25Ticks,
    Last100Ticks,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ActionFormState {
    Transit {
        action_id: ActionId,
        destination_id: u32,
        destination_name: String,
        selected_choice: usize,
        choices: Vec<ActionChoice<u32>>,
    },
    Build {
        location_id: u32,
        selected_choice: usize,
        choices: Vec<ActionChoice<InfrastructureKind>>,
    },
    Repair {
        location_id: u32,
        selected_choice: usize,
        choices: Vec<ActionChoice<InfrastructureKind>>,
    },
    Budget {
        selected_field: usize,
        upkeep: u32,
        research: u32,
        training: u32,
        agents: u32,
        total_available: u32,
    },
    Research {
        selected_choice: usize,
        choices: Vec<ActionChoice<ResearchBranch>>,
    },
    Training {
        selected_choice: usize,
        choices: Vec<ActionChoice<u8>>,
    },
    EventHistoryPreset {
        selected_choice: usize,
        choices: Vec<ActionChoice<EventHistoryChoice>>,
    },
    EventHistoryCustom {
        from_tick: u64,
    },
    Speed {
        selected_choice: usize,
        choices: Vec<ActionChoice<u64>>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FormSubmit {
    Command(PlayerScopedCommand),
    RefreshEvents(u64),
    OpenCustomEventHistory(u64),
    SetSpeed(u64),
}

pub(crate) fn derive_actions(
    frame: &PlayerFrameResponse,
    selected_location_index: usize,
) -> Vec<ActionItem> {
    let mut actions = Vec::new();
    if let Some(location) = frame.view.locations.get(selected_location_index) {
        actions.extend(location_actions(frame, location));
    }
    actions.extend(session_actions(frame));
    actions
}

pub(crate) fn default_selected_action_id(actions: &[ActionItem]) -> Option<ActionId> {
    actions.first().map(|action| action.id)
}

pub(crate) fn action_index(actions: &[ActionItem], action_id: ActionId) -> Option<usize> {
    actions.iter().position(|action| action.id == action_id)
}

pub(crate) fn action_by_id(actions: &[ActionItem], action_id: ActionId) -> Option<&ActionItem> {
    actions.iter().find(|action| action.id == action_id)
}

pub(crate) fn action_form_for_selected(
    action_id: ActionId,
    frame: &PlayerFrameResponse,
    selected_location_index: usize,
) -> Option<ActionFormState> {
    let location = frame.view.locations.get(selected_location_index)?;
    action_form_for_location(action_id, frame, location)
}

pub(crate) fn rebuild_form(
    form: &ActionFormState,
    frame: &PlayerFrameResponse,
) -> Option<ActionFormState> {
    match form {
        ActionFormState::Transit {
            action_id,
            destination_id,
            destination_name: _,
            selected_choice,
            choices,
        } => {
            let previous = choices.get(*selected_choice).map(|choice| choice.value);
            let location = location_by_id(frame, *destination_id)?;
            let mut rebuilt = action_form_for_location(*action_id, frame, location)?;
            if let Some(value) = previous
                && let ActionFormState::Transit {
                    selected_choice,
                    choices,
                    ..
                } = &mut rebuilt
                && let Some(index) = choices.iter().position(|choice| choice.value == value)
            {
                *selected_choice = index;
            }
            Some(rebuilt)
        }
        ActionFormState::Build {
            location_id,
            selected_choice,
            choices,
        } => {
            let previous = choices
                .get(*selected_choice)
                .map(|choice| choice.value.clone());
            let location = location_by_id(frame, *location_id)?;
            let mut rebuilt = action_form_for_location(ActionId::Build, frame, location)?;
            if let Some(value) = previous
                && let ActionFormState::Build {
                    selected_choice,
                    choices,
                    ..
                } = &mut rebuilt
                && let Some(index) = choices.iter().position(|choice| choice.value == value)
            {
                *selected_choice = index;
            }
            Some(rebuilt)
        }
        ActionFormState::Repair {
            location_id,
            selected_choice,
            choices,
        } => {
            let previous = choices
                .get(*selected_choice)
                .map(|choice| choice.value.clone());
            let location = location_by_id(frame, *location_id)?;
            let mut rebuilt = action_form_for_location(ActionId::Repair, frame, location)?;
            if let Some(value) = previous
                && let ActionFormState::Repair {
                    selected_choice,
                    choices,
                    ..
                } = &mut rebuilt
                && let Some(index) = choices.iter().position(|choice| choice.value == value)
            {
                *selected_choice = index;
            }
            Some(rebuilt)
        }
        ActionFormState::Budget {
            selected_field,
            upkeep,
            research,
            training,
            agents,
            total_available: _,
        } => Some(ActionFormState::Budget {
            selected_field: *selected_field,
            upkeep: *upkeep,
            research: *research,
            training: *training,
            agents: *agents,
            total_available: frame.view.throughput.available,
        }),
        ActionFormState::Research {
            selected_choice,
            choices,
        } => {
            let previous = choices.get(*selected_choice).map(|choice| choice.value);
            let mut rebuilt = research_form(frame);
            if let Some(value) = previous
                && let ActionFormState::Research {
                    selected_choice,
                    choices,
                } = &mut rebuilt
                && let Some(index) = choices.iter().position(|choice| choice.value == value)
            {
                *selected_choice = index;
            }
            Some(rebuilt)
        }
        ActionFormState::Training {
            selected_choice,
            choices,
        } => {
            let previous = choices.get(*selected_choice).map(|choice| choice.value);
            let mut rebuilt = training_form(frame);
            if let Some(value) = previous
                && let ActionFormState::Training {
                    selected_choice,
                    choices,
                } = &mut rebuilt
                && let Some(index) = choices.iter().position(|choice| choice.value == value)
            {
                *selected_choice = index;
            }
            Some(rebuilt)
        }
        ActionFormState::EventHistoryPreset {
            selected_choice,
            choices,
        } => Some(ActionFormState::EventHistoryPreset {
            selected_choice: *selected_choice,
            choices: choices.clone(),
        }),
        ActionFormState::EventHistoryCustom { from_tick } => {
            Some(ActionFormState::EventHistoryCustom {
                from_tick: *from_tick,
            })
        }
        ActionFormState::Speed {
            selected_choice,
            choices,
        } => {
            let previous = choices.get(*selected_choice).map(|choice| choice.value);
            let mut rebuilt = speed_form(frame);
            if let Some(value) = previous
                && let ActionFormState::Speed {
                    selected_choice,
                    choices,
                } = &mut rebuilt
                && let Some(index) = choices.iter().position(|choice| choice.value == value)
            {
                *selected_choice = index;
            }
            Some(rebuilt)
        }
    }
}

pub(crate) fn form_title(form: &ActionFormState) -> String {
    match form {
        ActionFormState::Transit {
            action_id,
            destination_name,
            ..
        } => format!("{} {}", action_label(*action_id), destination_name),
        ActionFormState::Build { location_id, .. } => format!("Build at #{location_id}"),
        ActionFormState::Repair { location_id, .. } => format!("Repair at #{location_id}"),
        ActionFormState::Budget { .. } => "Budget".to_owned(),
        ActionFormState::Research { .. } => "Research".to_owned(),
        ActionFormState::Training { .. } => "Training".to_owned(),
        ActionFormState::EventHistoryPreset { .. } => "Event History".to_owned(),
        ActionFormState::EventHistoryCustom { .. } => "Events From Tick".to_owned(),
        ActionFormState::Speed { .. } => "Set Speed".to_owned(),
    }
}

pub(crate) fn form_lines(form: &ActionFormState) -> Vec<String> {
    match form {
        ActionFormState::Transit {
            destination_id,
            selected_choice,
            choices,
            ..
        } => {
            let mut lines = vec![format!(
                "Choose an origin for destination #{destination_id}."
            )];
            lines.extend(render_choice_lines(choices, *selected_choice));
            lines.push("Up/Down choose  Enter confirm  Esc cancel".to_owned());
            lines
        }
        ActionFormState::Build {
            selected_choice,
            choices,
            ..
        } => {
            let mut lines = vec!["Choose infrastructure to construct.".to_owned()];
            lines.extend(render_choice_lines(choices, *selected_choice));
            lines.push("Up/Down choose  Enter confirm  Esc cancel".to_owned());
            lines
        }
        ActionFormState::Repair {
            selected_choice,
            choices,
            ..
        } => {
            let mut lines = vec!["Choose infrastructure to repair.".to_owned()];
            lines.extend(render_choice_lines(choices, *selected_choice));
            lines.push("Up/Down choose  Enter confirm  Esc cancel".to_owned());
            lines
        }
        ActionFormState::Budget {
            selected_field,
            upkeep,
            research,
            training,
            agents,
            total_available,
        } => {
            let total = upkeep + research + training + agents;
            let remaining = total_available.saturating_sub(total);
            let status = if total <= *total_available {
                format!("Reserved {total}/{total_available}. Remaining {remaining}.")
            } else {
                format!(
                    "Reserved {total}/{total_available}. Exceeds usable throughput by {}.",
                    total.saturating_sub(*total_available)
                )
            };
            vec![
                status,
                budget_field_line(0, *selected_field, "upkeep", *upkeep),
                budget_field_line(1, *selected_field, "research", *research),
                budget_field_line(2, *selected_field, "training", *training),
                budget_field_line(3, *selected_field, "agents", *agents),
                "Up/Down field  Left/Right adjust  Enter confirm  Esc cancel".to_owned(),
            ]
        }
        ActionFormState::Research {
            selected_choice,
            choices,
        } => {
            let mut lines = vec!["Choose a branch to advance to the next level.".to_owned()];
            lines.extend(render_choice_lines(choices, *selected_choice));
            lines.push("Up/Down choose  Enter confirm  Esc cancel".to_owned());
            lines
        }
        ActionFormState::Training {
            selected_choice,
            choices,
        } => {
            let mut lines = vec!["Choose a target tier for the next training run.".to_owned()];
            lines.extend(render_choice_lines(choices, *selected_choice));
            lines.push("Up/Down choose  Enter confirm  Esc cancel".to_owned());
            lines
        }
        ActionFormState::EventHistoryPreset {
            selected_choice,
            choices,
        } => {
            let mut lines = vec!["Choose an event history range.".to_owned()];
            lines.extend(render_choice_lines(choices, *selected_choice));
            lines.push("Up/Down choose  Enter confirm  Esc cancel".to_owned());
            lines
        }
        ActionFormState::EventHistoryCustom { from_tick } => vec![
            format!("Events from tick {from_tick}."),
            "Left/Right adjusts by 1. Up/Down adjusts by 10.".to_owned(),
            "Enter confirm  Esc back".to_owned(),
        ],
        ActionFormState::Speed {
            selected_choice,
            choices,
        } => {
            let mut lines = vec!["Choose a sandbox speed.".to_owned()];
            lines.extend(render_choice_lines(choices, *selected_choice));
            lines.push("Up/Down choose  Enter confirm  Esc cancel".to_owned());
            lines
        }
    }
}

pub(crate) fn selected_form_message(form: &ActionFormState) -> Option<String> {
    match form {
        ActionFormState::Transit {
            selected_choice,
            choices,
            ..
        } => choices.get(*selected_choice).map(choice_message),
        ActionFormState::Build {
            selected_choice,
            choices,
            ..
        } => choices.get(*selected_choice).map(choice_message),
        ActionFormState::Repair {
            selected_choice,
            choices,
            ..
        } => choices.get(*selected_choice).map(choice_message),
        ActionFormState::Research {
            selected_choice,
            choices,
        } => choices.get(*selected_choice).map(choice_message),
        ActionFormState::Training {
            selected_choice,
            choices,
        } => choices.get(*selected_choice).map(choice_message),
        ActionFormState::EventHistoryPreset {
            selected_choice,
            choices,
        } => choices.get(*selected_choice).map(choice_message),
        ActionFormState::Speed {
            selected_choice,
            choices,
        } => choices.get(*selected_choice).map(choice_message),
        ActionFormState::Budget { .. } | ActionFormState::EventHistoryCustom { .. } => None,
    }
}

pub(crate) fn form_next(form: &mut ActionFormState) {
    match form {
        ActionFormState::Transit {
            selected_choice,
            choices,
            ..
        } => cycle_choice_forward(selected_choice, choices.len()),
        ActionFormState::Build {
            selected_choice,
            choices,
            ..
        } => cycle_choice_forward(selected_choice, choices.len()),
        ActionFormState::Repair {
            selected_choice,
            choices,
            ..
        } => cycle_choice_forward(selected_choice, choices.len()),
        ActionFormState::Research {
            selected_choice,
            choices,
        } => cycle_choice_forward(selected_choice, choices.len()),
        ActionFormState::Training {
            selected_choice,
            choices,
        } => cycle_choice_forward(selected_choice, choices.len()),
        ActionFormState::EventHistoryPreset {
            selected_choice,
            choices,
        } => cycle_choice_forward(selected_choice, choices.len()),
        ActionFormState::Speed {
            selected_choice,
            choices,
        } => cycle_choice_forward(selected_choice, choices.len()),
        ActionFormState::Budget { selected_field, .. } => {
            *selected_field = (*selected_field + 1) % 4;
        }
        ActionFormState::EventHistoryCustom { from_tick } => {
            *from_tick = from_tick.saturating_add(10);
        }
    }
}

pub(crate) fn form_previous(form: &mut ActionFormState) {
    match form {
        ActionFormState::Transit {
            selected_choice,
            choices,
            ..
        } => cycle_choice_backward(selected_choice, choices.len()),
        ActionFormState::Build {
            selected_choice,
            choices,
            ..
        } => cycle_choice_backward(selected_choice, choices.len()),
        ActionFormState::Repair {
            selected_choice,
            choices,
            ..
        } => cycle_choice_backward(selected_choice, choices.len()),
        ActionFormState::Research {
            selected_choice,
            choices,
        } => cycle_choice_backward(selected_choice, choices.len()),
        ActionFormState::Training {
            selected_choice,
            choices,
        } => cycle_choice_backward(selected_choice, choices.len()),
        ActionFormState::EventHistoryPreset {
            selected_choice,
            choices,
        } => cycle_choice_backward(selected_choice, choices.len()),
        ActionFormState::Speed {
            selected_choice,
            choices,
        } => cycle_choice_backward(selected_choice, choices.len()),
        ActionFormState::Budget { selected_field, .. } => {
            *selected_field = (*selected_field + 3) % 4;
        }
        ActionFormState::EventHistoryCustom { from_tick } => {
            *from_tick = from_tick.saturating_sub(10);
        }
    }
}

pub(crate) fn form_adjust_left(form: &mut ActionFormState) {
    match form {
        ActionFormState::Budget {
            selected_field,
            upkeep,
            research,
            training,
            agents,
            ..
        } => {
            let field = match selected_field {
                0 => upkeep,
                1 => research,
                2 => training,
                _ => agents,
            };
            *field = field.saturating_sub(1);
        }
        ActionFormState::EventHistoryCustom { from_tick } => {
            *from_tick = from_tick.saturating_sub(1);
        }
        ActionFormState::Transit { .. }
        | ActionFormState::Build { .. }
        | ActionFormState::Repair { .. }
        | ActionFormState::Research { .. }
        | ActionFormState::Training { .. }
        | ActionFormState::EventHistoryPreset { .. }
        | ActionFormState::Speed { .. } => {}
    }
}

pub(crate) fn form_adjust_right(form: &mut ActionFormState) {
    match form {
        ActionFormState::Budget {
            selected_field,
            upkeep,
            research,
            training,
            agents,
            ..
        } => {
            let field = match selected_field {
                0 => upkeep,
                1 => research,
                2 => training,
                _ => agents,
            };
            *field = field.saturating_add(1);
        }
        ActionFormState::EventHistoryCustom { from_tick } => {
            *from_tick = from_tick.saturating_add(1);
        }
        ActionFormState::Transit { .. }
        | ActionFormState::Build { .. }
        | ActionFormState::Repair { .. }
        | ActionFormState::Research { .. }
        | ActionFormState::Training { .. }
        | ActionFormState::EventHistoryPreset { .. }
        | ActionFormState::Speed { .. } => {}
    }
}

pub(crate) fn form_back(form: &ActionFormState) -> Option<ActionFormState> {
    match form {
        ActionFormState::EventHistoryCustom { .. } => Some(event_history_form()),
        _ => None,
    }
}

pub(crate) fn submit_form(
    form: &ActionFormState,
    frame: &PlayerFrameResponse,
) -> Result<FormSubmit, String> {
    match form {
        ActionFormState::Transit {
            action_id,
            destination_id,
            selected_choice,
            choices,
            ..
        } => {
            let choice = choices
                .get(*selected_choice)
                .ok_or_else(|| "no origin is selected".to_owned())?;
            ensure_choice_enabled(choice)?;
            let origin = choice.value;
            let command = match action_id {
                ActionId::Survey => PlayerScopedCommand::Survey(TransitSpec {
                    origin,
                    destination: *destination_id,
                }),
                ActionId::Pacify => PlayerScopedCommand::Pacify(TransitSpec {
                    origin,
                    destination: *destination_id,
                }),
                ActionId::Claim => PlayerScopedCommand::Claim(TransitSpec {
                    origin,
                    destination: *destination_id,
                }),
                ActionId::Assault => PlayerScopedCommand::Assault(TransitSpec {
                    origin,
                    destination: *destination_id,
                }),
                ActionId::Strike => PlayerScopedCommand::Strike(TransitSpec {
                    origin,
                    destination: *destination_id,
                }),
                _ => return Err("unsupported transit action".to_owned()),
            };
            Ok(FormSubmit::Command(command))
        }
        ActionFormState::Build {
            location_id,
            selected_choice,
            choices,
        } => {
            let choice = choices
                .get(*selected_choice)
                .ok_or_else(|| "no infrastructure choice is selected".to_owned())?;
            ensure_choice_enabled(choice)?;
            Ok(FormSubmit::Command(PlayerScopedCommand::Build(
                ScopedInfrastructureArgs {
                    location: *location_id,
                    kind: choice.value.clone(),
                },
            )))
        }
        ActionFormState::Repair {
            location_id,
            selected_choice,
            choices,
        } => {
            let choice = choices
                .get(*selected_choice)
                .ok_or_else(|| "no infrastructure choice is selected".to_owned())?;
            ensure_choice_enabled(choice)?;
            Ok(FormSubmit::Command(PlayerScopedCommand::Repair(
                ScopedInfrastructureArgs {
                    location: *location_id,
                    kind: choice.value.clone(),
                },
            )))
        }
        ActionFormState::Budget {
            upkeep,
            research,
            training,
            agents,
            total_available,
            ..
        } => {
            let total = upkeep + research + training + agents;
            if total > *total_available {
                return Err(
                    "reserved throughput cannot exceed computed usable throughput".to_owned(),
                );
            }
            Ok(FormSubmit::Command(PlayerScopedCommand::Budget(
                ScopedBudgetArgs {
                    upkeep: *upkeep,
                    research: *research,
                    training: *training,
                    agents: *agents,
                },
            )))
        }
        ActionFormState::Research {
            selected_choice,
            choices,
        } => {
            let choice = choices
                .get(*selected_choice)
                .ok_or_else(|| "no research branch is selected".to_owned())?;
            ensure_choice_enabled(choice)?;
            let target_level = frame
                .view
                .research
                .level_for(choice.value)
                .saturating_add(1);
            Ok(FormSubmit::Command(PlayerScopedCommand::Research(
                ScopedResearchArgs {
                    branch: choice.value,
                    target_level,
                },
            )))
        }
        ActionFormState::Training {
            selected_choice,
            choices,
        } => {
            let choice = choices
                .get(*selected_choice)
                .ok_or_else(|| "no training tier is selected".to_owned())?;
            ensure_choice_enabled(choice)?;
            Ok(FormSubmit::Command(PlayerScopedCommand::Train(
                ScopedTrainArgs {
                    target_tier: choice.value,
                },
            )))
        }
        ActionFormState::EventHistoryPreset {
            selected_choice,
            choices,
        } => {
            let choice = choices
                .get(*selected_choice)
                .ok_or_else(|| "no event range is selected".to_owned())?;
            ensure_choice_enabled(choice)?;
            match choice.value {
                EventHistoryChoice::AllVisible => Ok(FormSubmit::RefreshEvents(0)),
                EventHistoryChoice::Last25Ticks => Ok(FormSubmit::RefreshEvents(
                    frame.summary.current_tick.0.saturating_sub(25),
                )),
                EventHistoryChoice::Last100Ticks => Ok(FormSubmit::RefreshEvents(
                    frame.summary.current_tick.0.saturating_sub(100),
                )),
                EventHistoryChoice::Custom => Ok(FormSubmit::OpenCustomEventHistory(
                    frame.summary.current_tick.0,
                )),
            }
        }
        ActionFormState::EventHistoryCustom { from_tick } => {
            Ok(FormSubmit::RefreshEvents(*from_tick))
        }
        ActionFormState::Speed {
            selected_choice,
            choices,
        } => {
            let choice = choices
                .get(*selected_choice)
                .ok_or_else(|| "no speed is selected".to_owned())?;
            ensure_choice_enabled(choice)?;
            Ok(FormSubmit::SetSpeed(choice.value))
        }
    }
}

pub(crate) fn event_history_form() -> ActionFormState {
    ActionFormState::EventHistoryPreset {
        selected_choice: 0,
        choices: vec![
            ActionChoice {
                label: "All visible".to_owned(),
                value: EventHistoryChoice::AllVisible,
                details: "Replay all visible events from tick 0.".to_owned(),
                availability: ActionAvailability::Enabled,
            },
            ActionChoice {
                label: "Last 25 ticks".to_owned(),
                value: EventHistoryChoice::Last25Ticks,
                details: "Replay visible events from the last 25 ticks.".to_owned(),
                availability: ActionAvailability::Enabled,
            },
            ActionChoice {
                label: "Last 100 ticks".to_owned(),
                value: EventHistoryChoice::Last100Ticks,
                details: "Replay visible events from the last 100 ticks.".to_owned(),
                availability: ActionAvailability::Enabled,
            },
            ActionChoice {
                label: "Custom".to_owned(),
                value: EventHistoryChoice::Custom,
                details: "Choose an explicit starting tick.".to_owned(),
                availability: ActionAvailability::Enabled,
            },
        ],
    }
}

pub(crate) fn event_history_custom_form(from_tick: u64) -> ActionFormState {
    ActionFormState::EventHistoryCustom { from_tick }
}

fn location_actions(frame: &PlayerFrameResponse, location: &LocationView) -> Vec<ActionItem> {
    match location_kind(location, frame.view.player_id) {
        LocationActionKind::Owned => vec![
            build_action(frame, location),
            repair_action(frame, location),
            relay_action(frame, location),
        ],
        LocationActionKind::Neutral => vec![
            transit_action(frame, location, ActionId::Survey),
            transit_action(frame, location, ActionId::Pacify),
            transit_action(frame, location, ActionId::Claim),
        ],
        LocationActionKind::Hostile => vec![
            transit_action(frame, location, ActionId::Survey),
            transit_action(frame, location, ActionId::Assault),
            transit_action(frame, location, ActionId::Strike),
        ],
    }
}

fn session_actions(frame: &PlayerFrameResponse) -> Vec<ActionItem> {
    let commands_locked = commands_locked(frame);
    let mut actions = vec![
        ActionItem {
            id: ActionId::Status,
            group: ActionGroup::Session,
            label: "Status Snapshot".to_owned(),
            preview: "Write the latest player-visible status into the output log.".to_owned(),
            availability: ActionAvailability::Enabled,
            opens_form: false,
        },
        ActionItem {
            id: ActionId::Map,
            group: ActionGroup::Session,
            label: "Map Snapshot".to_owned(),
            preview: "Write the current visible map into the output log.".to_owned(),
            availability: ActionAvailability::Enabled,
            opens_form: false,
        },
        ActionItem {
            id: ActionId::Events,
            group: ActionGroup::Session,
            label: "Event History…".to_owned(),
            preview: "Choose a visible event history range to replay in the output log.".to_owned(),
            availability: ActionAvailability::Enabled,
            opens_form: true,
        },
        ActionItem {
            id: ActionId::Budget,
            group: ActionGroup::Session,
            label: "Budget…".to_owned(),
            preview: format!(
                "Edit throughput reservations (upkeep={} research={} training={} agents={}).",
                frame.view.throughput.reserved_for_model_upkeep,
                frame.view.throughput.reserved_for_research,
                frame.view.throughput.reserved_for_training,
                frame.view.throughput.reserved_for_agents
            ),
            availability: if commands_locked {
                disabled(COMMANDS_LOCKED_REASON)
            } else {
                ActionAvailability::Enabled
            },
            opens_form: true,
        },
        ActionItem {
            id: ActionId::Research,
            group: ActionGroup::Session,
            label: "Research…".to_owned(),
            preview: "Choose the next research project to start.".to_owned(),
            availability: if commands_locked {
                disabled(COMMANDS_LOCKED_REASON)
            } else {
                ActionAvailability::Enabled
            },
            opens_form: true,
        },
        ActionItem {
            id: ActionId::Training,
            group: ActionGroup::Session,
            label: "Training…".to_owned(),
            preview: "Choose the next model tier training run.".to_owned(),
            availability: if commands_locked {
                disabled(COMMANDS_LOCKED_REASON)
            } else {
                ActionAvailability::Enabled
            },
            opens_form: true,
        },
    ];

    let ready = frame
        .seats
        .iter()
        .find(|seat| seat.player_id == frame.view.player_id)
        .map(|seat| seat.ready)
        .unwrap_or(false);
    actions.push(ActionItem {
        id: ActionId::Ready,
        group: ActionGroup::Session,
        label: if ready {
            "Mark Unready".to_owned()
        } else {
            "Mark Ready".to_owned()
        },
        preview: if ready {
            "Leave the lobby ready state.".to_owned()
        } else {
            "Mark this seat as ready in the lobby.".to_owned()
        },
        availability: if frame.runner.phase == SessionPhase::Lobby {
            ActionAvailability::Enabled
        } else {
            disabled("ready status can only change while the session is in the lobby")
        },
        opens_form: false,
    });

    actions.push(ActionItem {
        id: ActionId::PauseResume,
        group: ActionGroup::Session,
        label: if frame.runner.paused {
            "Resume Sandbox".to_owned()
        } else {
            "Pause Sandbox".to_owned()
        },
        preview: if frame.runner.paused {
            "Resume sandbox tick advancement.".to_owned()
        } else {
            "Pause sandbox tick advancement.".to_owned()
        },
        availability: sandbox_action_availability(
            frame,
            "pause is only available for sandbox sessions",
        ),
        opens_form: false,
    });

    actions.push(ActionItem {
        id: ActionId::Speed,
        group: ActionGroup::Session,
        label: "Set Speed…".to_owned(),
        preview: format!(
            "Choose a sandbox tick interval. Current {}ms.",
            frame.runner.tick_interval_ms
        ),
        availability: sandbox_action_availability(
            frame,
            "speed controls are only available for sandbox sessions",
        ),
        opens_form: true,
    });

    actions
}

fn build_action(frame: &PlayerFrameResponse, location: &LocationView) -> ActionItem {
    let preview = format!(
        "Choose infrastructure to construct at #{}.",
        location.location_id
    );
    ActionItem {
        id: ActionId::Build,
        group: ActionGroup::Location,
        label: "Build…".to_owned(),
        preview,
        availability: if commands_locked(frame) {
            disabled(COMMANDS_LOCKED_REASON)
        } else {
            ActionAvailability::Enabled
        },
        opens_form: true,
    }
}

fn repair_action(frame: &PlayerFrameResponse, location: &LocationView) -> ActionItem {
    let (enabled, reason) = if commands_locked(frame) {
        (false, Some(COMMANDS_LOCKED_REASON.to_owned()))
    } else if repair_choices(frame, location)
        .iter()
        .any(ActionChoice::is_enabled)
    {
        (true, None)
    } else {
        (
            false,
            Some(
                "repair can only be queued for degraded or offline infrastructure that is not already under repair"
                    .to_owned(),
            ),
        )
    };
    ActionItem {
        id: ActionId::Repair,
        group: ActionGroup::Location,
        label: "Repair…".to_owned(),
        preview: format!(
            "Choose damaged infrastructure to repair at #{}.",
            location.location_id
        ),
        availability: if enabled {
            ActionAvailability::Enabled
        } else {
            disabled(reason.unwrap_or_else(|| "repair unavailable".to_owned()))
        },
        opens_form: true,
    }
}

fn relay_action(frame: &PlayerFrameResponse, location: &LocationView) -> ActionItem {
    let availability = if commands_locked(frame) {
        disabled(COMMANDS_LOCKED_REASON)
    } else if location.relay_status.is_some() {
        ActionAvailability::Enabled
    } else {
        disabled("relay status is not available for the selected world")
    };
    let preview = match location.relay_status {
        Some(RelayStatus::Connected) => "Disconnect the selected world's relay uplink.",
        Some(RelayStatus::Disconnected) => "Connect the selected world's relay uplink.",
        None => "Relay status is unavailable for this world.",
    };

    ActionItem {
        id: ActionId::Relay,
        group: ActionGroup::Location,
        label: "Toggle Relay".to_owned(),
        preview: preview.to_owned(),
        availability,
        opens_form: false,
    }
}

fn transit_action(
    frame: &PlayerFrameResponse,
    location: &LocationView,
    action_id: ActionId,
) -> ActionItem {
    let availability = transit_target_availability(frame, location, action_id);
    ActionItem {
        id: action_id,
        group: ActionGroup::Location,
        label: format!("{}…", action_label(action_id)),
        preview: format!(
            "Choose an owned origin to {} #{}.",
            action_verb(action_id),
            location.location_id
        ),
        availability,
        opens_form: true,
    }
}

fn action_form_for_location(
    action_id: ActionId,
    frame: &PlayerFrameResponse,
    location: &LocationView,
) -> Option<ActionFormState> {
    match action_id {
        ActionId::Survey
        | ActionId::Pacify
        | ActionId::Claim
        | ActionId::Assault
        | ActionId::Strike => Some(ActionFormState::Transit {
            action_id,
            destination_id: location.location_id,
            destination_name: location.name.clone(),
            selected_choice: 0,
            choices: transit_origin_choices(frame, location, action_id),
        }),
        ActionId::Build => Some(ActionFormState::Build {
            location_id: location.location_id,
            selected_choice: 0,
            choices: build_choices(frame, location),
        }),
        ActionId::Repair => Some(ActionFormState::Repair {
            location_id: location.location_id,
            selected_choice: 0,
            choices: repair_choices(frame, location),
        }),
        ActionId::Budget => Some(ActionFormState::Budget {
            selected_field: 0,
            upkeep: frame.view.throughput.reserved_for_model_upkeep,
            research: frame.view.throughput.reserved_for_research,
            training: frame.view.throughput.reserved_for_training,
            agents: frame.view.throughput.reserved_for_agents,
            total_available: frame.view.throughput.available,
        }),
        ActionId::Research => Some(research_form(frame)),
        ActionId::Training => Some(training_form(frame)),
        ActionId::Events => Some(event_history_form()),
        ActionId::Speed => Some(speed_form(frame)),
        ActionId::Relay
        | ActionId::Status
        | ActionId::Map
        | ActionId::Ready
        | ActionId::PauseResume => None,
    }
}

fn research_form(frame: &PlayerFrameResponse) -> ActionFormState {
    ActionFormState::Research {
        selected_choice: 0,
        choices: research_choices(frame),
    }
}

fn training_form(frame: &PlayerFrameResponse) -> ActionFormState {
    ActionFormState::Training {
        selected_choice: 0,
        choices: training_choices(frame),
    }
}

fn speed_form(frame: &PlayerFrameResponse) -> ActionFormState {
    ActionFormState::Speed {
        selected_choice: 0,
        choices: SANDBOX_SPEED_CHOICES_MS
            .into_iter()
            .map(|value| speed_choice(value, frame))
            .collect(),
    }
}

fn transit_origin_choices(
    frame: &PlayerFrameResponse,
    destination: &LocationView,
    action_id: ActionId,
) -> Vec<ActionChoice<u32>> {
    let commands_locked = commands_locked(frame);
    owned_locations(frame)
        .into_iter()
        .map(|origin| {
            let route_eta = route_eta(frame, origin.location_id, destination.location_id);
            let mut availability = if commands_locked {
                disabled(COMMANDS_LOCKED_REASON)
            } else {
                ActionAvailability::Enabled
            };

            if availability.is_enabled() && route_eta.is_none() && action_id != ActionId::Survey {
                availability = disabled("no direct route exists between the requested origin and destination");
            }

            if availability.is_enabled() {
                match action_id {
                    ActionId::Assault => {
                        if !has_assault_staging(origin) {
                            availability = disabled(
                                "assaults require an operational military works or shipyard at the origin",
                            );
                        }
                    }
                    ActionId::Strike => {
                        if !has_assault_staging(origin) {
                            availability = disabled(
                                "strategic strikes require an operational military works or shipyard at the origin",
                            );
                        } else {
                            let warfare_level = frame.view.research.warfare_level;
                            let strike_cost = strategic_strike_cost(warfare_level);
                            if !materials_for_location(frame, origin).can_cover(&strike_cost) {
                                availability = disabled(
                                    "connected stockpiles cannot cover the requested strategic strike",
                                );
                            }
                        }
                    }
                    ActionId::Survey | ActionId::Pacify | ActionId::Claim => {}
                    _ => {}
                }
            }

            let mut details = match route_eta {
                Some(eta) => format!("eta {eta}"),
                None if action_id == ActionId::Survey => {
                    "long-range survey; eta determined on dispatch".to_owned()
                }
                None => "no direct route".to_owned(),
            };
            if action_id == ActionId::Strike {
                let cost = strategic_strike_cost(frame.view.research.warfare_level);
                details.push_str(&format!("  cost {}", format_stockpiles(&cost)));
            }

            ActionChoice {
                label: format!("#{} {}", origin.location_id, origin.name),
                value: origin.location_id,
                details,
                availability,
            }
        })
        .collect()
}

fn build_choices(
    frame: &PlayerFrameResponse,
    location: &LocationView,
) -> Vec<ActionChoice<InfrastructureKind>> {
    let build_capacity = location
        .build_capacity
        .clone()
        .unwrap_or(starforge_core::BuildCapacity::Standard);
    let has_environmental_hazard = location.has_environmental_hazard.unwrap_or(false);
    let industry_level = frame.view.research.industry_level;
    let connected = location
        .economy
        .as_ref()
        .map(|economy| economy.connected_to_empire)
        .unwrap_or(false);
    let stockpiles = if connected {
        frame.view.economy.connected_stockpiles.clone()
    } else {
        location.stockpiles.clone().unwrap_or_default()
    };

    buildable_infrastructure_kinds()
        .iter()
        .cloned()
        .map(|kind| {
            let preview = construction_preview(
                &kind,
                build_capacity.clone(),
                has_environmental_hazard,
                industry_level,
            );
            let mut availability = ActionAvailability::Enabled;
            if is_unique_infrastructure(&kind)
                && (location
                    .infrastructure
                    .as_ref()
                    .is_some_and(|items| items.iter().any(|item| item.kind == kind))
                    || location
                        .infrastructure_projects
                        .as_ref()
                        .is_some_and(|projects| {
                            projects.iter().any(|project| {
                                matches!(
                                    project.kind,
                                    InfrastructureProjectKind::Construction {
                                        infrastructure_kind: ref queued_kind,
                                    } if *queued_kind == kind
                                )
                            })
                        }))
            {
                availability = disabled(
                    "unique infrastructure cannot be constructed more than once per location",
                );
            } else if !stockpiles.can_cover(&preview.cost) {
                availability = disabled("connected stockpiles cannot cover the requested project");
            }

            ActionChoice {
                label: format!("{kind:?}"),
                value: kind,
                details: format!(
                    "cost {}  duration {}",
                    format_stockpiles(&preview.cost),
                    preview.duration_ticks
                ),
                availability,
            }
        })
        .collect()
}

fn repair_choices(
    frame: &PlayerFrameResponse,
    location: &LocationView,
) -> Vec<ActionChoice<InfrastructureKind>> {
    let Some(infrastructure) = &location.infrastructure else {
        return Vec::new();
    };
    let queued = queued_repair_targets(location);
    let build_capacity = location
        .build_capacity
        .clone()
        .unwrap_or(starforge_core::BuildCapacity::Standard);
    let has_environmental_hazard = location.has_environmental_hazard.unwrap_or(false);
    let industry_level = frame.view.research.industry_level;
    let connected = location
        .economy
        .as_ref()
        .map(|economy| economy.connected_to_empire)
        .unwrap_or(false);
    let stockpiles = if connected {
        frame.view.economy.connected_stockpiles.clone()
    } else {
        location.stockpiles.clone().unwrap_or_default()
    };

    let mut kinds = infrastructure
        .iter()
        .map(|item| item.kind.clone())
        .collect::<Vec<_>>();
    kinds.sort();
    kinds.dedup();

    kinds
        .into_iter()
        .map(|kind| {
            let target = infrastructure.iter().enumerate().find(|(index, item)| {
                item.kind == kind
                    && item.condition != InfrastructureCondition::Operational
                    && !queued.contains(index)
            });

            let (details, availability) = if let Some((_, infrastructure)) = target {
                let preview = repair_preview(
                    &kind,
                    &infrastructure.condition,
                    build_capacity.clone(),
                    has_environmental_hazard,
                    industry_level,
                );
                let availability = if stockpiles.can_cover(&preview.cost) {
                    ActionAvailability::Enabled
                } else {
                    disabled("connected stockpiles cannot cover the requested repair")
                };
                (
                    format!(
                        "{:?}  cost {}  duration {}",
                        infrastructure.condition,
                        format_stockpiles(&preview.cost),
                        preview.duration_ticks
                    ),
                    availability,
                )
            } else {
                (
                    "No damaged instance available.".to_owned(),
                    disabled(
                        "repair can only be queued for degraded or offline infrastructure that is not already under repair",
                    ),
                )
            };

            ActionChoice {
                label: format!("{kind:?}"),
                value: kind,
                details,
                availability,
            }
        })
        .collect()
}

fn research_choices(frame: &PlayerFrameResponse) -> Vec<ActionChoice<ResearchBranch>> {
    [
        ResearchBranch::Industry,
        ResearchBranch::Models,
        ResearchBranch::Warfare,
        ResearchBranch::Resilience,
    ]
    .into_iter()
    .map(|branch| {
        let target_level = frame.view.research.level_for(branch).saturating_add(1);
        let preview = research_preview(target_level);
        let mut availability = if commands_locked(frame) {
            disabled(COMMANDS_LOCKED_REASON)
        } else {
            ActionAvailability::Enabled
        };
        if availability.is_enabled() && frame.view.research.active_project.is_some() {
            availability = disabled("a research project is already active for this player");
        } else if availability.is_enabled() && target_level > 3 {
            availability = disabled("research targets must be between levels 1 and 3");
        } else if availability.is_enabled()
            && frame.view.throughput.reserved_for_research < preview.required_throughput
        {
            availability = disabled(
                "reserved research throughput is below the requirement for the requested level",
            );
        } else if availability.is_enabled() && !player_has_research_site(frame) {
            availability = disabled(
                "research requires at least one owned world with an operational command nexus and datacenter",
            );
        }

        ActionChoice {
            label: format!(
                "{} -> level {}",
                render_research_branch(branch),
                target_level
            ),
            value: branch,
            details: format!(
                "need {} research  duration {}",
                preview.required_throughput,
                preview.required_ticks
            ),
            availability,
        }
    })
    .collect()
}

fn training_choices(frame: &PlayerFrameResponse) -> Vec<ActionChoice<u8>> {
    (2..=5)
        .map(|target_tier| {
            let preview = training_preview(target_tier, frame.view.research.models_level);
            let mut availability = if commands_locked(frame) {
                disabled(COMMANDS_LOCKED_REASON)
            } else {
                ActionAvailability::Enabled
            };
            if availability.is_enabled() && frame.view.training.is_some() {
                availability = disabled("a training run is already active for this player");
            } else if availability.is_enabled()
                && target_tier != frame.view.model_tier.saturating_add(1)
            {
                availability = disabled("training runs must target the next unlocked tier");
            } else if availability.is_enabled()
                && frame.view.throughput.reserved_for_training < preview.required_throughput
            {
                availability = disabled(
                    "reserved training throughput is below the requirement for the requested tier",
                );
            } else if availability.is_enabled()
                && owned_locations(frame).len() < preview.minimum_worlds
            {
                availability = disabled(format!(
                    "training tier {target_tier} requires control of at least {} worlds",
                    preview.minimum_worlds
                ));
            } else if availability.is_enabled() && !player_has_research_site(frame) {
                availability = disabled(
                    "training requires at least one owned world with an operational command nexus and datacenter",
                );
            } else if availability.is_enabled()
                && target_tier >= 5
                && !player_has_ascension_site(frame)
            {
                availability = disabled(
                    "tier 5 ascension requires a connected owned world with an active command nexus and datacenter",
                );
            }

            ActionChoice {
                label: format!("Tier {target_tier}"),
                value: target_tier,
                details: format!(
                    "need {} training  duration {}  worlds {}",
                    preview.required_throughput, preview.required_ticks, preview.minimum_worlds
                ),
                availability,
            }
        })
        .collect()
}

fn speed_choice(value: u64, frame: &PlayerFrameResponse) -> ActionChoice<u64> {
    ActionChoice {
        label: format!("{value} ms"),
        value,
        details: if frame.runner.tick_interval_ms == value {
            "Current speed.".to_owned()
        } else {
            "Apply this sandbox tick interval.".to_owned()
        },
        availability: sandbox_action_availability(
            frame,
            "speed controls are only available for sandbox sessions",
        ),
    }
}

fn cycle_choice_forward(selected_choice: &mut usize, len: usize) {
    if len > 0 {
        *selected_choice = (*selected_choice + 1) % len;
    }
}

fn cycle_choice_backward(selected_choice: &mut usize, len: usize) {
    if len > 0 {
        *selected_choice = (*selected_choice + len - 1) % len;
    }
}

fn transit_target_availability(
    frame: &PlayerFrameResponse,
    location: &LocationView,
    action_id: ActionId,
) -> ActionAvailability {
    if commands_locked(frame) {
        return disabled(COMMANDS_LOCKED_REASON);
    }
    if owned_locations(frame).is_empty() {
        return disabled("no owned worlds are available as an origin");
    }

    match action_id {
        ActionId::Survey => {
            if location.visibility == LocationVisibility::Surveyed {
                disabled("the selected world has already been surveyed")
            } else {
                ActionAvailability::Enabled
            }
        }
        ActionId::Pacify => {
            if location.territory != TerritoryState::Neutral {
                disabled("pacification is currently limited to neutral worlds")
            } else if location.visibility != LocationVisibility::Surveyed {
                disabled(SURVEY_REQUIRED_REASON)
            } else if !location.hostile_remnant_present.unwrap_or(false) {
                disabled("pacification requires a hostile remnant at the destination")
            } else {
                ActionAvailability::Enabled
            }
        }
        ActionId::Claim => {
            if location.territory != TerritoryState::Neutral || location.controller.is_some() {
                disabled("claim expeditions require an unclaimed neutral world")
            } else if location.visibility != LocationVisibility::Surveyed {
                disabled(SURVEY_REQUIRED_REASON)
            } else if location.hostile_remnant_present.unwrap_or(false) {
                disabled("claiming requires hostile remnants to be cleared first")
            } else {
                ActionAvailability::Enabled
            }
        }
        ActionId::Assault => {
            if location.territory == TerritoryState::Contested {
                disabled("assault cannot be queued for a destination that is already contested")
            } else if location.visibility != LocationVisibility::Surveyed {
                disabled(SURVEY_REQUIRED_REASON)
            } else if location.hostile_remnant_present.unwrap_or(false) {
                disabled(
                    "assault expeditions currently require hostile remnants to be cleared first",
                )
            } else if location.controller == Some(frame.view.player_id)
                && location.territory == TerritoryState::Owned
            {
                disabled("assault requires a non-owned destination")
            } else if location.controller.is_none() {
                disabled("assault expeditions currently require an enemy-controlled destination")
            } else {
                ActionAvailability::Enabled
            }
        }
        ActionId::Strike => {
            if location.visibility != LocationVisibility::Surveyed {
                disabled(SURVEY_REQUIRED_REASON)
            } else if location.territory == TerritoryState::Destroyed {
                disabled("strategic strikes cannot target a destroyed world")
            } else if location.controller == Some(frame.view.player_id) {
                disabled("strategic strikes require a non-owned destination")
            } else if location.controller.is_none() {
                disabled("strategic strikes currently require an enemy-controlled destination")
            } else {
                ActionAvailability::Enabled
            }
        }
        _ => ActionAvailability::Enabled,
    }
}

fn sandbox_action_availability(
    frame: &PlayerFrameResponse,
    non_sandbox_reason: &str,
) -> ActionAvailability {
    if frame.runner.mode != SessionMode::Sandbox {
        disabled(non_sandbox_reason)
    } else if frame.runner.phase == SessionPhase::Finished {
        disabled("runner controls are unavailable after the match has finished")
    } else {
        ActionAvailability::Enabled
    }
}

fn choice_message<T>(choice: &ActionChoice<T>) -> String {
    if let Some(reason) = choice.availability.reason() {
        format!("{}: {}", choice.label, reason)
    } else {
        format!("{}: {}", choice.label, choice.details)
    }
}

fn render_choice_lines<T>(choices: &[ActionChoice<T>], selected_choice: usize) -> Vec<String> {
    choices
        .iter()
        .enumerate()
        .map(|(index, choice)| {
            let cursor = if index == selected_choice { ">" } else { " " };
            match &choice.availability {
                ActionAvailability::Enabled => {
                    format!("{cursor} {} - {}", choice.label, choice.details)
                }
                ActionAvailability::Disabled { reason } => {
                    format!("{cursor} {} - unavailable: {reason}", choice.label)
                }
            }
        })
        .collect()
}

fn budget_field_line(index: usize, selected_field: usize, label: &str, value: u32) -> String {
    let cursor = if index == selected_field { ">" } else { " " };
    format!("{cursor} {label}: {value}")
}

fn action_label(action_id: ActionId) -> &'static str {
    match action_id {
        ActionId::Survey => "Survey",
        ActionId::Pacify => "Pacify",
        ActionId::Claim => "Claim",
        ActionId::Assault => "Assault",
        ActionId::Strike => "Strike",
        ActionId::Build => "Build",
        ActionId::Repair => "Repair",
        ActionId::Relay => "Toggle Relay",
        ActionId::Status => "Status Snapshot",
        ActionId::Map => "Map Snapshot",
        ActionId::Events => "Event History",
        ActionId::Budget => "Budget",
        ActionId::Research => "Research",
        ActionId::Training => "Training",
        ActionId::Ready => "Ready",
        ActionId::PauseResume => "Pause/Resume",
        ActionId::Speed => "Set Speed",
    }
}

fn action_verb(action_id: ActionId) -> &'static str {
    match action_id {
        ActionId::Survey => "survey",
        ActionId::Pacify => "pacify",
        ActionId::Claim => "claim",
        ActionId::Assault => "assault",
        ActionId::Strike => "strike",
        ActionId::Build => "build",
        ActionId::Repair => "repair",
        ActionId::Relay => "toggle relay for",
        ActionId::Status => "inspect",
        ActionId::Map => "map",
        ActionId::Events => "inspect events for",
        ActionId::Budget => "budget",
        ActionId::Research => "research",
        ActionId::Training => "train",
        ActionId::Ready => "ready",
        ActionId::PauseResume => "pause or resume",
        ActionId::Speed => "set speed for",
    }
}

fn disabled(reason: impl Into<String>) -> ActionAvailability {
    ActionAvailability::Disabled {
        reason: reason.into(),
    }
}

fn ensure_choice_enabled<T>(choice: &ActionChoice<T>) -> Result<(), String> {
    match &choice.availability {
        ActionAvailability::Enabled => Ok(()),
        ActionAvailability::Disabled { reason } => Err(reason.clone()),
    }
}

fn commands_locked(frame: &PlayerFrameResponse) -> bool {
    frame.runner.mode == SessionMode::Competitive && frame.runner.phase == SessionPhase::Lobby
}

fn location_kind(location: &LocationView, player_id: PlayerId) -> LocationActionKind {
    if location.controller == Some(player_id) && location.territory == TerritoryState::Owned {
        LocationActionKind::Owned
    } else if location.territory == TerritoryState::Neutral && location.controller.is_none() {
        LocationActionKind::Neutral
    } else {
        LocationActionKind::Hostile
    }
}

fn owned_locations(frame: &PlayerFrameResponse) -> Vec<&LocationView> {
    frame
        .view
        .locations
        .iter()
        .filter(|location| {
            location.controller == Some(frame.view.player_id)
                && location.territory == TerritoryState::Owned
        })
        .collect()
}

fn route_eta(frame: &PlayerFrameResponse, origin_id: u32, destination_id: u32) -> Option<u32> {
    frame
        .known_routes
        .iter()
        .find(|route| {
            (route.from_location_id == origin_id && route.to_location_id == destination_id)
                || (route.from_location_id == destination_id && route.to_location_id == origin_id)
        })
        .map(|route| route.travel_time_ticks)
}

fn has_operational_infrastructure(location: &LocationView, kind: InfrastructureKind) -> bool {
    location.infrastructure.as_ref().is_some_and(|items| {
        items
            .iter()
            .any(|item| item.kind == kind && item.condition == InfrastructureCondition::Operational)
    })
}

fn has_assault_staging(location: &LocationView) -> bool {
    has_operational_infrastructure(location, InfrastructureKind::MilitaryWorks)
        || has_operational_infrastructure(location, InfrastructureKind::ShipyardRing)
}

fn player_has_research_site(frame: &PlayerFrameResponse) -> bool {
    owned_locations(frame).into_iter().any(|location| {
        has_operational_infrastructure(location, InfrastructureKind::CommandNexus)
            && has_operational_infrastructure(location, InfrastructureKind::Datacenter)
    })
}

fn player_has_ascension_site(frame: &PlayerFrameResponse) -> bool {
    owned_locations(frame).into_iter().any(|location| {
        location
            .economy
            .as_ref()
            .is_some_and(|economy| economy.connected_to_empire)
            && has_operational_infrastructure(location, InfrastructureKind::CommandNexus)
            && has_operational_infrastructure(location, InfrastructureKind::Datacenter)
    })
}

fn materials_for_location(
    frame: &PlayerFrameResponse,
    location: &LocationView,
) -> ResourceStockpiles {
    if location
        .economy
        .as_ref()
        .is_some_and(|economy| economy.connected_to_empire)
    {
        frame.view.economy.connected_stockpiles.clone()
    } else {
        location.stockpiles.clone().unwrap_or_default()
    }
}

fn queued_repair_targets(location: &LocationView) -> Vec<usize> {
    location
        .infrastructure_projects
        .as_ref()
        .map(|projects| {
            projects
                .iter()
                .filter_map(|project| match project.kind {
                    InfrastructureProjectKind::Repair { target_index, .. } => Some(target_index),
                    InfrastructureProjectKind::Construction { .. } => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn location_by_id(frame: &PlayerFrameResponse, location_id: u32) -> Option<&LocationView> {
    frame
        .view
        .locations
        .iter()
        .find(|location| location.location_id == location_id)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocationActionKind {
    Owned,
    Neutral,
    Hostile,
}

#[cfg(test)]
mod tests {
    use starforge_api::{PlayerAlertKind, RunnerStatus, SessionMode, SessionPhase};
    use starforge_core::{
        BuildCapacity, CommandCollapseState, EnergyPotential, InfrastructureCondition,
        InfrastructureKind, InfrastructureProjectState, InfrastructureState, LocationEconomyState,
        LocationKind, LocationView, LocationVisibility, PlayerEconomyState, PlayerId,
        PlayerResearchState, RelayStatus, ResearchBranch, ResourceRichness, ResourceStockpiles,
        TerritoryState, ThroughputBudget, TickId, VisibilityState,
    };

    use super::{ActionFormState, ActionId, action_by_id, derive_actions, repair_choices};

    #[test]
    fn healthy_owned_world_disables_repair_action() {
        let frame = running_frame();
        let actions = derive_actions(&frame, 0);
        let repair = action_by_id(&actions, ActionId::Repair).expect("repair action");
        assert!(!repair.availability.is_enabled());
        assert!(repair.summary().contains("repair can only be queued"));
    }

    #[test]
    fn damaged_world_enables_repair_choice() {
        let mut frame = running_frame();
        frame.view.locations[0].infrastructure = Some(vec![InfrastructureState {
            kind: InfrastructureKind::Datacenter,
            tier: 1,
            condition: InfrastructureCondition::Offline,
            wear: 100,
        }]);

        let choices = repair_choices(&frame, &frame.view.locations[0]);
        assert!(choices.iter().any(|choice| {
            choice.value == InfrastructureKind::Datacenter && choice.availability.is_enabled()
        }));
    }

    #[test]
    fn observed_neutral_world_enables_survey_and_disables_claim() {
        let frame = running_frame();
        let actions = derive_actions(&frame, 1);
        let survey = action_by_id(&actions, ActionId::Survey).expect("survey");
        let claim = action_by_id(&actions, ActionId::Claim).expect("claim");
        assert!(survey.availability.is_enabled());
        assert!(!claim.availability.is_enabled());
    }

    #[test]
    fn surveyed_remnant_world_enables_pacify_and_blocks_claim() {
        let mut frame = running_frame();
        frame.view.locations[1].visibility = LocationVisibility::Surveyed;
        frame.view.locations[1].hostile_remnant_present = Some(true);

        let actions = derive_actions(&frame, 1);
        let pacify = action_by_id(&actions, ActionId::Pacify).expect("pacify");
        let claim = action_by_id(&actions, ActionId::Claim).expect("claim");
        assert!(pacify.availability.is_enabled());
        assert!(claim.summary().contains("hostile remnants"));
    }

    #[test]
    fn surveyed_clear_world_enables_claim() {
        let mut frame = running_frame();
        frame.view.locations[1].visibility = LocationVisibility::Surveyed;
        frame.view.locations[1].hostile_remnant_present = Some(false);

        let actions = derive_actions(&frame, 1);
        let claim = action_by_id(&actions, ActionId::Claim).expect("claim");
        assert!(claim.availability.is_enabled());
    }

    #[test]
    fn enemy_unsurveyed_world_blocks_assault_and_strike() {
        let frame = make_enemy_frame();
        let actions = derive_actions(&frame, 1);
        let assault = action_by_id(&actions, ActionId::Assault).expect("assault");
        let strike = action_by_id(&actions, ActionId::Strike).expect("strike");
        assert!(assault.summary().contains("survey"));
        assert!(strike.summary().contains("survey"));
    }

    #[test]
    fn survey_origin_picker_allows_long_range_dispatch_without_known_route() {
        let mut frame = running_frame();
        frame.known_routes.clear();
        let form =
            super::action_form_for_selected(ActionId::Survey, &frame, 1).expect("survey form");
        let ActionFormState::Transit { choices, .. } = form else {
            panic!("expected transit form");
        };
        assert!(
            choices
                .iter()
                .all(|choice| choice.availability.is_enabled())
        );
        assert!(
            choices
                .iter()
                .all(|choice| choice.summary().contains("long-range survey"))
        );
    }

    #[test]
    fn budget_form_tracks_overallocation() {
        let frame = crate::live_test_frame();
        let form =
            super::action_form_for_selected(ActionId::Budget, &frame, 0).expect("budget form");
        let ActionFormState::Budget {
            total_available, ..
        } = form
        else {
            panic!("expected budget form");
        };
        assert_eq!(total_available, 50);
    }

    #[test]
    fn research_form_disables_when_project_active() {
        let mut frame = crate::live_test_frame();
        frame.view.research = PlayerResearchState {
            active_project: Some(starforge_core::ResearchProjectState {
                branch: ResearchBranch::Models,
                target_level: 1,
                progress_ticks: 2,
                required_ticks: 8,
                required_research_throughput: 16,
            }),
            ..PlayerResearchState::default()
        };
        let form =
            super::action_form_for_selected(ActionId::Research, &frame, 0).expect("research form");
        let ActionFormState::Research { choices, .. } = form else {
            panic!("expected research form");
        };
        assert!(
            choices
                .iter()
                .all(|choice| !choice.availability.is_enabled())
        );
    }

    #[test]
    fn training_form_shows_world_count_requirement() {
        let mut frame = running_frame();
        frame.view.model_tier = 2;
        frame.view.throughput.reserved_for_training = 35;
        frame.view.locations[0].infrastructure = Some(vec![
            InfrastructureState {
                kind: InfrastructureKind::CommandNexus,
                tier: 1,
                condition: InfrastructureCondition::Operational,
                wear: 0,
            },
            InfrastructureState {
                kind: InfrastructureKind::Datacenter,
                tier: 1,
                condition: InfrastructureCondition::Operational,
                wear: 0,
            },
        ]);

        let form =
            super::action_form_for_selected(ActionId::Training, &frame, 0).expect("training form");
        let ActionFormState::Training { choices, .. } = form else {
            panic!("expected training form");
        };
        let tier_three = choices
            .iter()
            .find(|choice| choice.value == 3)
            .expect("tier 3");
        assert!(
            tier_three
                .summary()
                .contains("requires control of at least 2 worlds")
        );
    }

    #[test]
    fn speed_form_uses_slower_presets() {
        let mut frame = running_frame();
        frame.runner.tick_interval_ms = super::SANDBOX_SPEED_CHOICES_MS[0];

        let form = super::action_form_for_selected(ActionId::Speed, &frame, 0).expect("speed form");
        let ActionFormState::Speed { choices, .. } = form else {
            panic!("expected speed form");
        };

        let values = choices
            .iter()
            .map(|choice| choice.value)
            .collect::<Vec<_>>();
        assert_eq!(values, super::SANDBOX_SPEED_CHOICES_MS);
        assert_eq!(choices[0].details, "Current speed.");
    }

    fn running_frame() -> starforge_api::PlayerFrameResponse {
        let mut frame = crate::live_test_frame();
        frame.runner.mode = SessionMode::Sandbox;
        frame.runner.phase = SessionPhase::Running;
        frame.runner.pause_allowed = true;
        frame.runner.speed_change_allowed = true;
        frame.runner.paused = false;
        frame.alerts.clear();
        if let Some(economy) = frame.view.locations[0].economy.as_mut() {
            economy.connected_to_empire = true;
        }
        frame
    }

    fn make_enemy_frame() -> starforge_api::PlayerFrameResponse {
        starforge_api::PlayerFrameResponse {
            session_id: starforge_core::SessionId::new(9),
            summary: starforge_api::LiveSessionSummary {
                scenario_name: "enemy".to_owned(),
                current_tick: TickId::new(20),
                victory: starforge_core::VictoryState::Ongoing,
            },
            seats: vec![
                starforge_api::PlayerSeat {
                    player_id: PlayerId::new(1),
                    claimed: true,
                    ready: false,
                },
                starforge_api::PlayerSeat {
                    player_id: PlayerId::new(2),
                    claimed: true,
                    ready: false,
                },
            ],
            runner: RunnerStatus {
                mode: SessionMode::Sandbox,
                phase: SessionPhase::Running,
                tick_interval_ms: 250,
                pause_allowed: true,
                speed_change_allowed: true,
                paused: false,
            },
            state_hash: 7,
            next_event_index: 0,
            view: starforge_core::PlayerStateView {
                tick_id: TickId::new(20),
                player_id: PlayerId::new(1),
                model_tier: 1,
                economy: PlayerEconomyState {
                    total_connected_energy: 60,
                    total_connected_datacenter_capacity: 50,
                    usable_throughput: 50,
                    connected_stockpiles: ResourceStockpiles {
                        common_materials: 500,
                        volatiles: 120,
                        rare_materials: 60,
                    },
                    disconnected_owned_location_ids: Vec::new(),
                },
                throughput: ThroughputBudget {
                    reserved_for_model_upkeep: 0,
                    reserved_for_research: 20,
                    reserved_for_training: 20,
                    reserved_for_agents: 0,
                    available: 50,
                },
                research: PlayerResearchState::default(),
                training: None,
                collapse: CommandCollapseState::Stable,
                visibility: VisibilityState::default(),
                locations: vec![
                    LocationView {
                        location_id: 1,
                        name: "Helios".to_owned(),
                        visibility: LocationVisibility::Owned,
                        territory: TerritoryState::Owned,
                        controller: Some(PlayerId::new(1)),
                        contesting_players: None,
                        pacification_ticks_remaining: Some(0),
                        kind: Some(LocationKind::HabitablePlanet),
                        resource_richness: Some(ResourceRichness::Rich),
                        energy_potential: Some(EnergyPotential::High),
                        build_capacity: Some(BuildCapacity::Expansive),
                        relay_status: Some(RelayStatus::Connected),
                        orbital_slots: Some(3),
                        has_environmental_hazard: Some(false),
                        infrastructure: Some(vec![
                            InfrastructureState {
                                kind: InfrastructureKind::CommandNexus,
                                tier: 1,
                                condition: InfrastructureCondition::Operational,
                                wear: 0,
                            },
                            InfrastructureState {
                                kind: InfrastructureKind::Datacenter,
                                tier: 1,
                                condition: InfrastructureCondition::Operational,
                                wear: 0,
                            },
                            InfrastructureState {
                                kind: InfrastructureKind::MilitaryWorks,
                                tier: 1,
                                condition: InfrastructureCondition::Operational,
                                wear: 0,
                            },
                        ]),
                        infrastructure_projects: Some(Vec::<InfrastructureProjectState>::new()),
                        economy: Some(LocationEconomyState::default()),
                        stockpiles: Some(ResourceStockpiles::default()),
                        hostile_remnant_present: Some(false),
                    },
                    LocationView {
                        location_id: 2,
                        name: "Enemy".to_owned(),
                        visibility: LocationVisibility::Observed,
                        territory: TerritoryState::Owned,
                        controller: Some(PlayerId::new(2)),
                        contesting_players: None,
                        pacification_ticks_remaining: Some(0),
                        kind: Some(LocationKind::HabitablePlanet),
                        resource_richness: Some(ResourceRichness::Moderate),
                        energy_potential: Some(EnergyPotential::Moderate),
                        build_capacity: Some(BuildCapacity::Standard),
                        relay_status: Some(RelayStatus::Connected),
                        orbital_slots: Some(2),
                        has_environmental_hazard: Some(false),
                        infrastructure: Some(Vec::new()),
                        infrastructure_projects: Some(Vec::<InfrastructureProjectState>::new()),
                        economy: Some(LocationEconomyState::default()),
                        stockpiles: Some(ResourceStockpiles::default()),
                        hostile_remnant_present: Some(false),
                    },
                ],
                routes: vec![starforge_core::LocationConnection {
                    from_location_id: 1,
                    to_location_id: 2,
                    travel_time_ticks: 30,
                }],
                transits: Vec::new(),
            },
            events: Vec::new(),
            alerts: vec![starforge_api::PlayerAlert {
                kind: PlayerAlertKind::Survey,
                title: "enemy world observed".to_owned(),
                tick_id: TickId::new(20),
                location_id: Some(2),
            }],
            known_routes: vec![starforge_api::KnownRouteView {
                from_location_id: 1,
                to_location_id: 2,
                travel_time_ticks: 30,
            }],
        }
    }
}
