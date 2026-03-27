use crate::{BuildCapacity, InfrastructureCondition, InfrastructureKind, ResourceStockpiles};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectPreview {
    pub cost: ResourceStockpiles,
    pub duration_ticks: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResearchPreview {
    pub required_throughput: u32,
    pub required_ticks: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrainingPreview {
    pub required_throughput: u32,
    pub required_ticks: u32,
    pub minimum_worlds: usize,
}

const BUILDABLE_INFRASTRUCTURE_KINDS: [InfrastructureKind; 7] = [
    InfrastructureKind::MiningSite,
    InfrastructureKind::EnergyProducer,
    InfrastructureKind::Datacenter,
    InfrastructureKind::RelayUplink,
    InfrastructureKind::ShipyardRing,
    InfrastructureKind::MilitaryWorks,
    InfrastructureKind::GroundDefenseSite,
];

pub fn buildable_infrastructure_kinds() -> &'static [InfrastructureKind] {
    &BUILDABLE_INFRASTRUCTURE_KINDS
}

pub const fn is_unique_infrastructure(infrastructure_kind: &InfrastructureKind) -> bool {
    matches!(
        infrastructure_kind,
        InfrastructureKind::CommandNexus | InfrastructureKind::RelayUplink
    )
}

pub fn construction_preview(
    infrastructure_kind: &InfrastructureKind,
    build_capacity: BuildCapacity,
    has_environmental_hazard: bool,
    industry_level: u8,
) -> ProjectPreview {
    ProjectPreview {
        cost: construction_cost(infrastructure_kind, industry_level),
        duration_ticks: construction_duration(
            build_capacity,
            has_environmental_hazard,
            infrastructure_kind,
            industry_level,
        ),
    }
}

pub fn repair_preview(
    infrastructure_kind: &InfrastructureKind,
    condition: &InfrastructureCondition,
    build_capacity: BuildCapacity,
    has_environmental_hazard: bool,
    industry_level: u8,
) -> ProjectPreview {
    ProjectPreview {
        cost: repair_cost(infrastructure_kind, condition, industry_level),
        duration_ticks: repair_duration(
            build_capacity,
            has_environmental_hazard,
            condition,
            industry_level,
        ),
    }
}

pub fn research_preview(target_level: u8) -> ResearchPreview {
    ResearchPreview {
        required_throughput: research_throughput_requirement(target_level),
        required_ticks: research_duration_ticks(target_level),
    }
}

pub fn training_preview(target_tier: u8, models_level: u8) -> TrainingPreview {
    TrainingPreview {
        required_throughput: training_throughput_requirement(target_tier),
        required_ticks: training_duration_ticks(target_tier, models_level),
        minimum_worlds: minimum_worlds_for_tier(target_tier),
    }
}

pub fn strategic_strike_cost(warfare_level: u8) -> ResourceStockpiles {
    scale_cost_by_percent(
        ResourceStockpiles {
            common_materials: 120,
            volatiles: 80,
            rare_materials: 30,
        },
        100u32
            .saturating_sub(u32::from(warfare_level).saturating_mul(10))
            .max(70),
    )
}

fn training_throughput_requirement(target_tier: u8) -> u32 {
    match target_tier {
        2 => 20,
        3 => 35,
        4 => 50,
        5 => 70,
        _ => u32::MAX,
    }
}

fn training_duration_ticks(target_tier: u8, models_level: u8) -> u32 {
    let base_ticks = match target_tier {
        2 => 32,
        3 => 48,
        4 => 72,
        5 => 96,
        _ => u32::MAX,
    };
    let modifier_percent = match models_level {
        0 => 100,
        1 => 90,
        2 => 80,
        _ => 70,
    };

    base_ticks
        .saturating_mul(modifier_percent)
        .saturating_div(100)
        .max(1)
}

fn research_throughput_requirement(target_level: u8) -> u32 {
    match target_level {
        1 => 16,
        2 => 24,
        3 => 34,
        _ => u32::MAX,
    }
}

fn research_duration_ticks(target_level: u8) -> u32 {
    match target_level {
        1 => 8,
        2 => 12,
        3 => 16,
        _ => u32::MAX,
    }
}

fn minimum_worlds_for_tier(target_tier: u8) -> usize {
    match target_tier {
        2 => 1,
        3 => 2,
        4 => 3,
        5 => 4,
        _ => usize::MAX,
    }
}

fn repair_cost(
    infrastructure_kind: &InfrastructureKind,
    condition: &InfrastructureCondition,
    industry_level: u8,
) -> ResourceStockpiles {
    let base_cost = match infrastructure_kind {
        InfrastructureKind::CommandNexus => ResourceStockpiles {
            common_materials: 60,
            volatiles: 20,
            rare_materials: 10,
        },
        InfrastructureKind::MiningSite => ResourceStockpiles {
            common_materials: 30,
            volatiles: 10,
            rare_materials: 0,
        },
        InfrastructureKind::EnergyProducer => ResourceStockpiles {
            common_materials: 45,
            volatiles: 15,
            rare_materials: 4,
        },
        InfrastructureKind::Datacenter => ResourceStockpiles {
            common_materials: 40,
            volatiles: 10,
            rare_materials: 4,
        },
        InfrastructureKind::RelayUplink => ResourceStockpiles {
            common_materials: 25,
            volatiles: 8,
            rare_materials: 2,
        },
        InfrastructureKind::ShipyardRing => ResourceStockpiles {
            common_materials: 70,
            volatiles: 20,
            rare_materials: 10,
        },
        InfrastructureKind::MilitaryWorks => ResourceStockpiles {
            common_materials: 60,
            volatiles: 16,
            rare_materials: 8,
        },
        InfrastructureKind::GroundDefenseSite => ResourceStockpiles {
            common_materials: 35,
            volatiles: 10,
            rare_materials: 4,
        },
    };

    let multiplier = match condition {
        InfrastructureCondition::Operational => 0,
        InfrastructureCondition::Degraded => 1,
        InfrastructureCondition::Offline => 2,
    };

    scale_cost_by_percent(
        ResourceStockpiles {
            common_materials: base_cost.common_materials.saturating_mul(multiplier),
            volatiles: base_cost.volatiles.saturating_mul(multiplier),
            rare_materials: base_cost.rare_materials.saturating_mul(multiplier),
        },
        100u32
            .saturating_sub(u32::from(industry_level).saturating_mul(10))
            .max(70),
    )
}

fn repair_duration(
    build_capacity: BuildCapacity,
    has_environmental_hazard: bool,
    condition: &InfrastructureCondition,
    industry_level: u8,
) -> u32 {
    let base_duration: i32 = match condition {
        InfrastructureCondition::Operational => 0,
        InfrastructureCondition::Degraded => 3,
        InfrastructureCondition::Offline => 5,
    };
    let build_adjustment = match build_capacity {
        BuildCapacity::Constrained => 1,
        BuildCapacity::Standard => 0,
        BuildCapacity::Expansive => -1,
    };
    let hazard_adjustment = if has_environmental_hazard { 1 } else { 0 };
    let industry_adjustment = -(i32::from(industry_level));

    (base_duration + build_adjustment + hazard_adjustment + industry_adjustment).max(1) as u32
}

fn construction_cost(
    infrastructure_kind: &InfrastructureKind,
    industry_level: u8,
) -> ResourceStockpiles {
    let base_cost = match infrastructure_kind {
        InfrastructureKind::MiningSite => ResourceStockpiles {
            common_materials: 70,
            volatiles: 20,
            rare_materials: 0,
        },
        InfrastructureKind::EnergyProducer => ResourceStockpiles {
            common_materials: 90,
            volatiles: 30,
            rare_materials: 8,
        },
        InfrastructureKind::Datacenter => ResourceStockpiles {
            common_materials: 80,
            volatiles: 20,
            rare_materials: 8,
        },
        InfrastructureKind::RelayUplink => ResourceStockpiles {
            common_materials: 50,
            volatiles: 15,
            rare_materials: 4,
        },
        InfrastructureKind::ShipyardRing => ResourceStockpiles {
            common_materials: 120,
            volatiles: 40,
            rare_materials: 16,
        },
        InfrastructureKind::MilitaryWorks => ResourceStockpiles {
            common_materials: 110,
            volatiles: 35,
            rare_materials: 12,
        },
        InfrastructureKind::GroundDefenseSite => ResourceStockpiles {
            common_materials: 90,
            volatiles: 30,
            rare_materials: 10,
        },
        InfrastructureKind::CommandNexus => ResourceStockpiles::default(),
    };

    scale_cost_by_percent(
        base_cost,
        100u32
            .saturating_sub(u32::from(industry_level).saturating_mul(10))
            .max(70),
    )
}

fn construction_duration(
    build_capacity: BuildCapacity,
    has_environmental_hazard: bool,
    infrastructure_kind: &InfrastructureKind,
    industry_level: u8,
) -> u32 {
    let base_duration: i32 = match infrastructure_kind {
        InfrastructureKind::MiningSite => 3,
        InfrastructureKind::EnergyProducer => 4,
        InfrastructureKind::Datacenter => 4,
        InfrastructureKind::RelayUplink => 3,
        InfrastructureKind::ShipyardRing => 6,
        InfrastructureKind::MilitaryWorks => 5,
        InfrastructureKind::GroundDefenseSite => 5,
        InfrastructureKind::CommandNexus => 0,
    };
    let build_adjustment = match build_capacity {
        BuildCapacity::Constrained => 1,
        BuildCapacity::Standard => 0,
        BuildCapacity::Expansive => -1,
    };
    let hazard_adjustment = if has_environmental_hazard { 1 } else { 0 };
    let industry_adjustment = -(i32::from(industry_level));

    (base_duration + build_adjustment + hazard_adjustment + industry_adjustment).max(1) as u32
}

fn scale_cost_by_percent(cost: ResourceStockpiles, percent: u32) -> ResourceStockpiles {
    ResourceStockpiles {
        common_materials: cost
            .common_materials
            .saturating_mul(percent)
            .saturating_div(100),
        volatiles: cost.volatiles.saturating_mul(percent).saturating_div(100),
        rare_materials: cost
            .rare_materials
            .saturating_mul(percent)
            .saturating_div(100),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ProjectPreview, ResearchPreview, TrainingPreview, buildable_infrastructure_kinds,
        construction_preview, is_unique_infrastructure, repair_preview, research_preview,
        strategic_strike_cost, training_preview,
    };
    use crate::{BuildCapacity, InfrastructureCondition, InfrastructureKind, ResourceStockpiles};

    #[test]
    fn construction_preview_matches_expected_values() {
        assert_eq!(
            construction_preview(
                &InfrastructureKind::Datacenter,
                BuildCapacity::Standard,
                false,
                0,
            ),
            ProjectPreview {
                cost: ResourceStockpiles {
                    common_materials: 80,
                    volatiles: 20,
                    rare_materials: 8,
                },
                duration_ticks: 4,
            }
        );
    }

    #[test]
    fn repair_preview_matches_expected_values() {
        assert_eq!(
            repair_preview(
                &InfrastructureKind::Datacenter,
                &InfrastructureCondition::Offline,
                BuildCapacity::Expansive,
                false,
                1,
            ),
            ProjectPreview {
                cost: ResourceStockpiles {
                    common_materials: 72,
                    volatiles: 18,
                    rare_materials: 7,
                },
                duration_ticks: 3,
            }
        );
    }

    #[test]
    fn research_preview_matches_expected_values() {
        assert_eq!(
            research_preview(2),
            ResearchPreview {
                required_throughput: 24,
                required_ticks: 12,
            }
        );
    }

    #[test]
    fn training_preview_matches_expected_values() {
        assert_eq!(
            training_preview(4, 2),
            TrainingPreview {
                required_throughput: 50,
                required_ticks: 57,
                minimum_worlds: 3,
            }
        );
    }

    #[test]
    fn helper_surfaces_buildable_and_unique_kinds() {
        assert!(buildable_infrastructure_kinds().contains(&InfrastructureKind::MiningSite));
        assert!(!buildable_infrastructure_kinds().contains(&InfrastructureKind::CommandNexus));
        assert!(is_unique_infrastructure(&InfrastructureKind::RelayUplink));
        assert!(!is_unique_infrastructure(&InfrastructureKind::Datacenter));
    }

    #[test]
    fn strategic_strike_cost_scales_with_warfare_level() {
        assert_eq!(
            strategic_strike_cost(2),
            ResourceStockpiles {
                common_materials: 96,
                volatiles: 64,
                rare_materials: 24,
            }
        );
    }
}
