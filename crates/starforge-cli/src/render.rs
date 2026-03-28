use starforge_api::KnownRouteView;
use starforge_core::{
    CommandCollapseState, EventKind, InfrastructureProjectViewKind, LocationConnection,
    LocationView, LocationVisibility, PlayerId, PlayerStateView, ResearchBranch,
    ResourceStockpiles, TransitKind, VictoryState,
};

pub(crate) fn render_status_lines(
    session_label: &str,
    player_id: PlayerId,
    view: &PlayerStateView,
    victory: &VictoryState,
) -> Vec<String> {
    let owned_count = view
        .locations
        .iter()
        .filter(|location| location.visibility == LocationVisibility::Owned)
        .count();

    let mut lines = vec![
        session_label.to_owned(),
        format!("Tick: {}", view.tick_id.0),
        format!("Victory: {}", render_victory(victory)),
        format!("Player: P{}", player_id.0),
        format!("Model tier: {}", view.model_tier),
        format!("Collapse: {}", render_collapse(&view.collapse)),
        format!("Owned worlds: {}", owned_count),
        format!(
            "Throughput: available={} upkeep={} research={} training={} agents={}",
            view.throughput.available,
            view.throughput.reserved_for_model_upkeep,
            view.throughput.reserved_for_research,
            view.throughput.reserved_for_training,
            view.throughput.reserved_for_agents
        ),
        format!(
            "Research: industry={} models={} warfare={} resilience={}",
            view.research.industry_level,
            view.research.models_level,
            view.research.warfare_level,
            view.research.resilience_level
        ),
        format!(
            "Connected economy: energy={} datacenter={} usable_throughput={}",
            view.economy.total_connected_energy,
            view.economy.total_connected_datacenter_capacity,
            view.economy.usable_throughput
        ),
        format!(
            "Connected stockpiles: {}",
            format_stockpiles(&view.economy.connected_stockpiles)
        ),
    ];

    match &view.research.active_project {
        Some(project) => lines.push(format!(
            "Research project: {} level {} progress {}/{} requiring {} research throughput",
            render_research_branch(project.branch),
            project.target_level,
            project.progress_ticks,
            project.required_ticks,
            project.required_research_throughput
        )),
        None => lines.push("Research project: none".to_owned()),
    }

    match &view.training {
        Some(training) => {
            let site_suffix = training
                .ascension_site_location_id
                .map(|location_id| format!(" site=#{location_id}"))
                .unwrap_or_default();
            lines.push(format!(
                "Training: tier {} progress {}/{} requiring {} training throughput{}",
                training.target_tier,
                training.progress_ticks,
                training.required_ticks,
                training.required_training_throughput,
                site_suffix
            ));
        }
        None => lines.push("Training: none".to_owned()),
    }

    if view.transits.is_empty() {
        lines.push("Transits: none".to_owned());
    } else {
        lines.push("Transits:".to_owned());
        lines.extend(view.transits.iter().map(|transit| {
            format!(
                "  #{} {} {} -> {} eta={}",
                transit.transit_id,
                render_transit_kind(&transit.kind),
                transit.origin_id,
                transit.destination_id,
                transit.eta_tick.0
            )
        }));
    }

    lines
}

pub(crate) fn render_map_lines(
    player_id: PlayerId,
    tick_id: u64,
    victory: &VictoryState,
    locations: &[LocationView],
    known_routes: &[KnownRouteView],
) -> Vec<String> {
    let mut lines = vec![format!(
        "Map for P{} at tick {} ({})",
        player_id.0,
        tick_id,
        render_victory(victory)
    )];
    lines.extend(locations.iter().map(render_location));
    lines.push(String::new());
    lines.push("Reachable routes from currently known worlds:".to_owned());
    lines.extend(known_routes.iter().map(|route| {
        format!(
            "  {} <-> {} (eta {})",
            route.from_location_id, route.to_location_id, route.travel_time_ticks
        )
    }));
    lines
}

pub(crate) fn render_known_routes(
    connections: &[LocationConnection],
    locations: &[LocationView],
) -> Vec<KnownRouteView> {
    let known_location_ids = locations
        .iter()
        .filter(|location| location.visibility != LocationVisibility::Obscured)
        .map(|location| location.location_id)
        .collect::<Vec<_>>();

    connections
        .iter()
        .filter(|connection| {
            known_location_ids.contains(&connection.from_location_id)
                || known_location_ids.contains(&connection.to_location_id)
        })
        .map(|connection| KnownRouteView {
            from_location_id: connection.from_location_id,
            to_location_id: connection.to_location_id,
            travel_time_ticks: connection.travel_time_ticks,
        })
        .collect()
}

pub(crate) fn render_location(location: &LocationView) -> String {
    let visibility = format!("{:?}", location.visibility);
    let territory = format!("{:?}", location.territory);
    let mut summary = format!(
        "#{:>2} {:<20} {:<9} territory={}",
        location.location_id, location.name, visibility, territory,
    );

    if let Some(controller) = location.controller {
        summary.push_str(&format!(" controller=P{}", controller.0));
    }
    if let Some(kind) = &location.kind {
        summary.push_str(&format!(" kind={kind:?}"));
    }
    if let Some(resource_richness) = &location.resource_richness {
        summary.push_str(&format!(" resources={resource_richness:?}"));
    }
    if let Some(energy_potential) = &location.energy_potential {
        summary.push_str(&format!(" energy_potential={energy_potential:?}"));
    }
    if let Some(build_capacity) = &location.build_capacity {
        summary.push_str(&format!(" build={build_capacity:?}"));
    }
    if let Some(has_environmental_hazard) = location.has_environmental_hazard {
        summary.push_str(&format!(" hazard={has_environmental_hazard}"));
    }
    if let Some(hostile_remnant_present) = location.hostile_remnant_present {
        summary.push_str(&format!(" remnant={hostile_remnant_present}"));
    }
    if let Some(contesting_players) = &location.contesting_players
        && !contesting_players.is_empty()
    {
        summary.push_str(&format!(" contesting={contesting_players:?}"));
    }
    if let Some(pacification_ticks_remaining) = location.pacification_ticks_remaining
        && pacification_ticks_remaining > 0
    {
        summary.push_str(&format!(" pacification={pacification_ticks_remaining}"));
    }
    if let Some(relay_status) = &location.relay_status {
        summary.push_str(&format!(" relay={relay_status:?}"));
    }
    if let Some(infrastructure) = &location.infrastructure {
        summary.push_str(&format!(
            " infra=[{}]",
            infrastructure
                .iter()
                .map(render_infrastructure_family)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if let Some(projects) = &location.infrastructure_projects
        && !projects.is_empty()
    {
        summary.push_str(&format!(
            " projects=[{}]",
            projects
                .iter()
                .map(render_infrastructure_project)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if let Some(economy) = &location.economy {
        summary.push_str(&format!(
            " economy=(energy={} dc={} throughput={} connected={})",
            economy.generated_energy,
            economy.datacenter_capacity,
            economy.empire_usable_throughput,
            economy.connected_to_empire
        ));
    }
    if let Some(stockpiles) = &location.stockpiles {
        summary.push_str(&format!(" stockpiles={}", format_stockpiles(stockpiles)));
    }

    summary
}

pub(crate) fn render_location_details(location: &LocationView) -> String {
    let mut lines = vec![
        format!("#{} {}", location.location_id, location.name),
        format!(
            "visibility={:?} territory={:?}",
            location.visibility, location.territory
        ),
    ];

    if let Some(controller) = location.controller {
        lines.push(format!("controller=P{}", controller.0));
    }

    let mut attributes = Vec::new();
    if let Some(kind) = &location.kind {
        attributes.push(format!("kind={kind:?}"));
    }
    if let Some(resource_richness) = &location.resource_richness {
        attributes.push(format!("resources={resource_richness:?}"));
    }
    if let Some(energy_potential) = &location.energy_potential {
        attributes.push(format!("energy={energy_potential:?}"));
    }
    if let Some(build_capacity) = &location.build_capacity {
        attributes.push(format!("build={build_capacity:?}"));
    }
    if let Some(orbital_slots) = location.orbital_slots {
        attributes.push(format!("orbital_slots={orbital_slots}"));
    }
    if !attributes.is_empty() {
        lines.push(attributes.join(" "));
    }

    let mut status = Vec::new();
    if let Some(relay_status) = &location.relay_status {
        status.push(format!("relay={relay_status:?}"));
    }
    if let Some(has_environmental_hazard) = location.has_environmental_hazard {
        status.push(format!("hazard={has_environmental_hazard}"));
    }
    if let Some(hostile_remnant_present) = location.hostile_remnant_present {
        status.push(format!("remnant={hostile_remnant_present}"));
    }
    if let Some(pacification_ticks_remaining) = location.pacification_ticks_remaining
        && pacification_ticks_remaining > 0
    {
        status.push(format!("pacification={pacification_ticks_remaining}"));
    }
    if !status.is_empty() {
        lines.push(status.join(" "));
    }

    match &location.infrastructure {
        Some(infrastructure) if !infrastructure.is_empty() => {
            lines.push("Infrastructure:".to_owned());
            lines.extend(
                infrastructure
                    .iter()
                    .map(|family| format!("  {}", render_infrastructure_family(family))),
            );
        }
        Some(_) => lines.push("Infrastructure: none".to_owned()),
        None => lines.push("Infrastructure: unavailable".to_owned()),
    }

    match &location.infrastructure_projects {
        Some(projects) if !projects.is_empty() => {
            lines.push("Projects:".to_owned());
            lines.extend(
                projects
                    .iter()
                    .map(|project| format!("  {}", render_infrastructure_project(project))),
            );
        }
        Some(_) => {}
        None => {}
    }

    if let Some(economy) = &location.economy {
        lines.push(format!(
            "Economy: energy={} dc={} throughput={} connected={}",
            economy.generated_energy,
            economy.datacenter_capacity,
            economy.empire_usable_throughput,
            economy.connected_to_empire
        ));
    }
    if let Some(stockpiles) = &location.stockpiles {
        lines.push(format!("Stockpiles: {}", format_stockpiles(stockpiles)));
    }

    lines.join("\n")
}

pub(crate) fn render_event(event: &EventKind) -> String {
    match event {
        EventKind::SessionCreated { player_ids, seed } => {
            format!(
                "session created for players {:?} seed={}",
                player_ids, seed.0
            )
        }
        EventKind::TickAdvanced { tick_id } => format!("tick advanced to {}", tick_id.0),
        EventKind::CommandAccepted {
            command,
            apply_at_tick,
        } => format!(
            "command accepted {:?} apply_at={}",
            command, apply_at_tick.0
        ),
        EventKind::CommandApplied { command } => format!("command applied {:?}", command),
        EventKind::CommandRejected { command, error } => format!(
            "command rejected {:?}: {} ({})",
            command, error.message, error.code
        ),
        EventKind::ThroughputBudgetSet {
            reserved_for_model_upkeep,
            reserved_for_research,
            reserved_for_training,
            reserved_for_agents,
            available,
        } => format!(
            "throughput budget upkeep={} research={} training={} agents={} available={}",
            reserved_for_model_upkeep,
            reserved_for_research,
            reserved_for_training,
            reserved_for_agents,
            available
        ),
        EventKind::EconomyUpdated {
            player_id,
            total_connected_energy,
            total_connected_datacenter_capacity,
            usable_throughput,
        } => format!(
            "economy updated for P{} energy={} datacenter={} throughput={}",
            player_id.0,
            total_connected_energy,
            total_connected_datacenter_capacity,
            usable_throughput
        ),
        EventKind::AgentAssigned {
            role,
            scope,
            reserved_throughput,
        } => format!(
            "agent assigned role={} scope={} throughput={}",
            role, scope, reserved_throughput
        ),
        EventKind::LocationRegistered { location_id, name } => {
            format!("location registered #{} {}", location_id, name)
        }
        EventKind::RelayStatusChanged {
            location_id,
            relay_status,
        } => format!(
            "relay status changed at #{} to {:?}",
            location_id, relay_status
        ),
        EventKind::InfrastructureConditionChanged {
            location_id,
            kind,
            condition,
        } => format!(
            "infrastructure condition changed at #{} {:?} -> {:?}",
            location_id, kind, condition
        ),
        EventKind::InfrastructureRepairQueued {
            location_id,
            kind,
            duration_ticks,
            cost,
        } => format!(
            "repair queued at #{} {:?} duration={} cost={}",
            location_id,
            kind,
            duration_ticks,
            format_stockpiles(cost)
        ),
        EventKind::InfrastructureRepairCompleted { location_id, kind } => {
            format!("repair completed at #{} {:?}", location_id, kind)
        }
        EventKind::InfrastructureDevelopmentQueued {
            location_id,
            kind,
            target_level,
            duration_ticks,
            cost,
        } => format!(
            "development queued at #{} {:?} -> L{} duration={} cost={}",
            location_id,
            kind,
            target_level,
            duration_ticks,
            format_stockpiles(cost)
        ),
        EventKind::InfrastructureDevelopmentCompleted {
            location_id,
            kind,
            achieved_level,
        } => {
            format!(
                "development completed at #{} {:?} -> L{}",
                location_id, kind, achieved_level
            )
        }
        EventKind::TransitDispatched {
            transit_id,
            origin_id,
            destination_id,
            eta_tick,
            kind,
        } => format!(
            "transit #{} {} {} -> {} eta={}",
            transit_id,
            render_transit_kind(kind),
            origin_id,
            destination_id,
            eta_tick.0
        ),
        EventKind::TransitArrived {
            transit_id,
            destination_id,
            kind,
        } => format!(
            "transit #{} {} arrived at {}",
            transit_id,
            render_transit_kind(kind),
            destination_id
        ),
        EventKind::LocationSurveyed { location_id } => {
            format!("location #{} surveyed", location_id)
        }
        EventKind::HostileRemnantCleared { location_id } => {
            format!("hostile remnant cleared at #{}", location_id)
        }
        EventKind::LocationClaimed {
            location_id,
            player_id,
        } => format!("location #{} claimed by P{}", location_id, player_id.0),
        EventKind::LocationContested {
            location_id,
            attacker_id,
            defender_id,
        } => match defender_id {
            Some(defender_id) => format!(
                "location #{} contested by P{} against P{}",
                location_id, attacker_id.0, defender_id.0
            ),
            None => format!("location #{} contested by P{}", location_id, attacker_id.0),
        },
        EventKind::AssaultRepelled {
            location_id,
            attacker_id,
            defender_id,
        } => format!(
            "assault on #{} by P{} repelled by P{}",
            location_id, attacker_id.0, defender_id.0
        ),
        EventKind::LocationCaptured {
            location_id,
            attacker_id,
            defender_id,
            pacification_ticks,
        } => format!(
            "location #{} captured by P{} from P{}; pacification={}",
            location_id, attacker_id.0, defender_id.0, pacification_ticks
        ),
        EventKind::PacificationCompleted {
            location_id,
            player_id,
        } => format!(
            "pacification completed at #{} for P{}",
            location_id, player_id.0
        ),
        EventKind::StrategicStrikeIntercepted {
            location_id,
            attacker_id,
            defender_id,
        } => format!(
            "strategic strike on #{} by P{} intercepted by P{}",
            location_id, attacker_id.0, defender_id.0
        ),
        EventKind::LocationDestroyed {
            location_id,
            attacker_id,
            defender_id,
        } => format!(
            "location #{} destroyed by P{} against P{}",
            location_id, attacker_id.0, defender_id.0
        ),
        EventKind::TrainingRunStarted {
            target_tier,
            required_training_throughput,
            required_ticks,
        } => format!(
            "training run started for tier {} requiring {} throughput for {} ticks",
            target_tier, required_training_throughput, required_ticks
        ),
        EventKind::TrainingRunCompleted { achieved_tier } => {
            format!("training run completed; achieved tier {}", achieved_tier)
        }
        EventKind::ResearchProjectStarted {
            branch,
            target_level,
            required_research_throughput,
            required_ticks,
        } => format!(
            "research started for {} level {} requiring {} throughput for {} ticks",
            render_research_branch(*branch),
            target_level,
            required_research_throughput,
            required_ticks
        ),
        EventKind::ResearchProjectCompleted {
            branch,
            achieved_level,
        } => format!(
            "research completed for {} level {}",
            render_research_branch(*branch),
            achieved_level
        ),
        EventKind::AscensionStarted {
            player_id,
            location_id,
            required_training_throughput,
            required_ticks,
        } => format!(
            "ascension started for P{} at #{} requiring {} throughput for {} ticks",
            player_id.0, location_id, required_training_throughput, required_ticks
        ),
        EventKind::AscensionInterrupted {
            player_id,
            location_id,
            reason,
        } => format!(
            "ascension interrupted for P{} at #{} ({})",
            player_id.0, location_id, reason
        ),
        EventKind::CommandCollapseStarted {
            player_id,
            ticks_remaining,
        } => format!(
            "command collapse started for P{} with {} ticks remaining",
            player_id.0, ticks_remaining
        ),
        EventKind::CommandCollapseRecovered { player_id } => {
            format!("command collapse recovered for P{}", player_id.0)
        }
        EventKind::PlayerDefeated { player_id, reason } => {
            format!("player P{} defeated ({})", player_id.0, reason)
        }
        EventKind::VictoryDeclared { winner, reason } => {
            format!("victory declared for P{} ({})", winner.0, reason)
        }
    }
}

fn render_infrastructure_family(family: &starforge_core::InfrastructureFamilyView) -> String {
    let mut states = Vec::new();
    if family.degraded_levels > 0 {
        states.push(format!("{} degraded", family.degraded_levels));
    }
    if family.offline_levels > 0 {
        states.push(format!("{} offline", family.offline_levels));
    }

    if states.is_empty() {
        format!("{:?} L{}", family.kind, family.level)
    } else {
        format!(
            "{:?} L{} ({})",
            family.kind,
            family.level,
            states.join(", ")
        )
    }
}

fn render_infrastructure_project(project: &starforge_core::InfrastructureProjectView) -> String {
    let project_kind = match project.project_kind {
        InfrastructureProjectViewKind::Repair => "repair",
        InfrastructureProjectViewKind::Development => "develop",
    };
    format!(
        "{} {:?} -> L{} {}/{}",
        project_kind,
        project.kind,
        project.target_level,
        project.total_ticks.saturating_sub(project.remaining_ticks),
        project.total_ticks
    )
}

pub(crate) fn render_victory(victory: &VictoryState) -> String {
    match victory {
        VictoryState::Ongoing => "victory=ongoing".to_owned(),
        VictoryState::Won { winner } => format!("victory=won by P{}", winner.0),
    }
}

pub(crate) fn render_collapse(collapse: &CommandCollapseState) -> String {
    match collapse {
        CommandCollapseState::Stable => "stable".to_owned(),
        CommandCollapseState::Collapsing { ticks_remaining } => {
            format!("collapsing ({ticks_remaining} ticks remaining)")
        }
        CommandCollapseState::Defeated => "defeated".to_owned(),
    }
}

pub(crate) fn format_stockpiles(stockpiles: &ResourceStockpiles) -> String {
    format!(
        "common={} volatiles={} rare={}",
        stockpiles.common_materials, stockpiles.volatiles, stockpiles.rare_materials
    )
}

pub(crate) fn render_transit_kind(kind: &TransitKind) -> &'static str {
    match kind {
        TransitKind::Survey => "survey",
        TransitKind::Pacification => "pacify",
        TransitKind::Claim => "claim",
        TransitKind::Assault => "assault",
        TransitKind::StrategicStrike => "strategic-strike",
    }
}

pub(crate) fn render_research_branch(branch: ResearchBranch) -> &'static str {
    match branch {
        ResearchBranch::Industry => "industry",
        ResearchBranch::Models => "models",
        ResearchBranch::Warfare => "warfare",
        ResearchBranch::Resilience => "resilience",
    }
}

#[cfg(test)]
mod tests {
    use starforge_core::{
        CommandCollapseState, EventKind, PlayerId, PlayerResearchState, ResourceStockpiles,
        ThroughputBudget, TickId, VictoryState, VisibilityState,
    };

    use super::{render_event, render_status_lines};

    #[test]
    fn status_lines_include_research_budget_and_project() {
        let mut frame = crate::live_test_frame();
        let view = &mut frame.view;
        view.tick_id = TickId::new(7);
        view.player_id = PlayerId::new(1);
        view.throughput = ThroughputBudget {
            reserved_for_model_upkeep: 4,
            reserved_for_research: 9,
            reserved_for_training: 12,
            reserved_for_agents: 3,
            available: 20,
        };
        view.economy.connected_stockpiles = ResourceStockpiles {
            common_materials: 10,
            volatiles: 5,
            rare_materials: 2,
        };
        view.collapse = CommandCollapseState::Stable;
        view.visibility = VisibilityState::default();
        view.research = PlayerResearchState {
            industry_level: 1,
            models_level: 2,
            warfare_level: 0,
            resilience_level: 3,
            active_project: Some(starforge_core::ResearchProjectState {
                branch: starforge_core::ResearchBranch::Models,
                target_level: 3,
                progress_ticks: 4,
                required_ticks: 12,
                required_research_throughput: 16,
            }),
        };

        let lines = render_status_lines(
            "Session: test",
            PlayerId::new(1),
            view,
            &VictoryState::Ongoing,
        );

        assert!(lines.iter().any(|line| line.contains("research=9")));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("Research: industry=1 models=2 warfare=0 resilience=3"))
        );
        assert!(lines.iter().any(|line| {
            line.contains("Research project: models level 3 progress 4/12 requiring 16")
        }));
    }

    #[test]
    fn render_event_formats_research_events() {
        let started = render_event(&EventKind::ResearchProjectStarted {
            branch: starforge_core::ResearchBranch::Industry,
            target_level: 2,
            required_research_throughput: 24,
            required_ticks: 18,
        });
        let completed = render_event(&EventKind::ResearchProjectCompleted {
            branch: starforge_core::ResearchBranch::Resilience,
            achieved_level: 1,
        });

        assert_eq!(
            started,
            "research started for industry level 2 requiring 24 throughput for 18 ticks"
        );
        assert_eq!(completed, "research completed for resilience level 1");
    }
}
