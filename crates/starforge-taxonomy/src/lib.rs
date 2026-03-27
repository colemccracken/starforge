use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fmt, fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use starforge_content::{
    ContentError, RulesetDocument, ScenarioDocument, load_ruleset_document, load_scenario_document,
};
use starforge_core::{
    BuildCapacity, CommandDiscriminant, EnergyPotential, EventDiscriminant, HostileRemnantKind,
    InfrastructureCondition, InfrastructureKind, LocationKind, LocationVisibility, RelayStatus,
    ResourceRichness, StrategicPosition, TerritoryState, ThreatLevel, TransitKind,
};
use strum::IntoEnumIterator;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaxonomyDocument {
    pub title: String,
    pub ruleset_name: String,
    pub scenario_name: String,
    pub ruleset_path: String,
    pub scenario_path: String,
    pub root_ids: Vec<String>,
    pub entries: Vec<TaxonomyEntry>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaxonomyEntry {
    pub id: String,
    pub title: String,
    pub kind: TaxonomyEntryKind,
    pub status: TaxonomyStatus,
    pub parent_id: Option<String>,
    pub child_ids: Vec<String>,
    pub domain_id: String,
    pub domain_title: String,
    pub summary: String,
    pub reference_sources: Vec<TaxonomySourceRef>,
    pub implementation_sources: Vec<TaxonomySourceRef>,
    pub behavior_sources: Vec<TaxonomySourceRef>,
    pub runtime_values: Vec<TaxonomyRuntimeValue>,
    pub related_ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaxonomyStatus {
    Implemented,
    Partial,
    Planned,
    Draft,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaxonomyEntryKind {
    Section,
    Concept,
    Command,
    Event,
    EnumCategory,
    ContentField,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaxonomySourceKind {
    ReferenceSection,
    ImplementationKey,
    BehaviorCoverage,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaxonomySourceRef {
    pub kind: TaxonomySourceKind,
    pub label: String,
    pub target: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaxonomyRuntimeValue {
    pub binding: String,
    pub value: Value,
}

#[derive(Debug)]
pub enum TaxonomyError {
    Io(std::io::Error),
    Yaml(serde_yaml::Error),
    Json(serde_json::Error),
    Content(ContentError),
    Validation(Vec<String>),
}

impl fmt::Display for TaxonomyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "failed to read taxonomy data: {error}"),
            Self::Yaml(error) => write!(f, "failed to parse taxonomy yaml: {error}"),
            Self::Json(error) => write!(f, "failed to serialize taxonomy schema: {error}"),
            Self::Content(error) => write!(f, "{error}"),
            Self::Validation(errors) => {
                write!(f, "taxonomy validation failed: {}", errors.join("; "))
            }
        }
    }
}

impl std::error::Error for TaxonomyError {}

impl From<std::io::Error> for TaxonomyError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_yaml::Error> for TaxonomyError {
    fn from(error: serde_yaml::Error) -> Self {
        Self::Yaml(error)
    }
}

impl From<serde_json::Error> for TaxonomyError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<ContentError> for TaxonomyError {
    fn from(error: ContentError) -> Self {
        Self::Content(error)
    }
}

#[derive(Clone, Debug, Deserialize)]
struct RegistryFile {
    entries: Vec<TaxonomyEntryDefinition>,
}

#[derive(Clone, Debug, Deserialize)]
struct TaxonomyEntryDefinition {
    id: String,
    title: String,
    kind: TaxonomyEntryKind,
    parent_id: Option<String>,
    summary: String,
    status: Option<TaxonomyStatus>,
    #[serde(default)]
    reference_sections: Vec<String>,
    #[serde(default)]
    implementation_keys: Vec<String>,
    #[serde(default)]
    behavior_ids: Vec<String>,
    #[serde(default)]
    content_bindings: Vec<String>,
    #[serde(default)]
    related_ids: Vec<String>,
}

#[derive(Clone, Debug)]
struct IndexedDefinition {
    index: usize,
    definition: TaxonomyEntryDefinition,
}

#[derive(Clone, Debug)]
struct ValidationContext {
    headings: BTreeSet<String>,
    implementation_keys: BTreeSet<String>,
    exact_coverage_keys: BTreeSet<String>,
    behavior_ids: BTreeSet<String>,
    content_paths: BTreeSet<String>,
}

pub fn build_taxonomy_document(
    configured_ruleset_path: impl AsRef<Path>,
    configured_scenario_path: impl AsRef<Path>,
) -> Result<TaxonomyDocument, TaxonomyError> {
    let configured_ruleset_path = configured_ruleset_path.as_ref();
    let configured_scenario_path = configured_scenario_path.as_ref();
    let ruleset = load_ruleset_document(configured_ruleset_path)?;
    let scenario = load_scenario_document(configured_scenario_path)?;
    let definitions = load_registry_definitions()?;
    let validation = validation_context()?;
    validate_registry(&definitions, &validation)?;

    let definition_map: HashMap<&str, &IndexedDefinition> = definitions
        .iter()
        .map(|definition| (definition.definition.id.as_str(), definition))
        .collect();
    let children_map = children_map(&definitions);
    let mut status_cache = HashMap::new();
    let ruleset_json = serde_json::to_value(&ruleset)?;
    let scenario_json = serde_json::to_value(&scenario)?;

    let mut entries = definitions
        .iter()
        .map(|indexed| {
            let definition = &indexed.definition;
            let status = derive_status(
                &definition.id,
                &definition_map,
                &children_map,
                &mut status_cache,
            );
            let (domain_id, domain_title) =
                domain_for_entry(&definition.id, &definition_map, &children_map);
            TaxonomyEntry {
                id: definition.id.clone(),
                title: definition.title.clone(),
                kind: definition.kind,
                status,
                parent_id: definition.parent_id.clone(),
                child_ids: sorted_child_ids(&definition.id, &children_map, &definition_map),
                domain_id,
                domain_title,
                summary: definition.summary.clone(),
                reference_sources: definition
                    .reference_sections
                    .iter()
                    .map(|section| TaxonomySourceRef {
                        kind: TaxonomySourceKind::ReferenceSection,
                        label: section.clone(),
                        target: format!(
                            "STARFORGE_REFERENCE.md#{}",
                            slugify_markdown_heading(section)
                        ),
                    })
                    .collect(),
                implementation_sources: definition
                    .implementation_keys
                    .iter()
                    .map(|key| TaxonomySourceRef {
                        kind: TaxonomySourceKind::ImplementationKey,
                        label: key.clone(),
                        target: key.clone(),
                    })
                    .collect(),
                behavior_sources: definition
                    .behavior_ids
                    .iter()
                    .map(|id| TaxonomySourceRef {
                        kind: TaxonomySourceKind::BehaviorCoverage,
                        label: id.clone(),
                        target: id.clone(),
                    })
                    .collect(),
                runtime_values: definition
                    .content_bindings
                    .iter()
                    .filter_map(|binding| {
                        runtime_value_for_binding(binding, &ruleset_json, &scenario_json).map(
                            |value| TaxonomyRuntimeValue {
                                binding: binding.clone(),
                                value,
                            },
                        )
                    })
                    .collect(),
                related_ids: definition.related_ids.clone(),
            }
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| definition_map[entry.id.as_str()].index);

    Ok(TaxonomyDocument {
        title: "Starforge Taxonomy".to_owned(),
        ruleset_name: ruleset.name,
        scenario_name: scenario.name,
        ruleset_path: configured_ruleset_path.display().to_string(),
        scenario_path: configured_scenario_path.display().to_string(),
        root_ids: definitions
            .iter()
            .filter(|definition| definition.definition.parent_id.is_none())
            .map(|definition| definition.definition.id.clone())
            .collect(),
        entries,
    })
}

pub fn behavior_coverage_ids() -> &'static [&'static str] {
    &[
        "behavior.economy.throughput_computation",
        "behavior.economy.relay_disconnect_zeroes_empire_throughput",
        "behavior.infrastructure.repair_project_completion",
        "behavior.infrastructure.construction_project_completion",
        "behavior.intel.survey_transit_marks_location_surveyed",
        "behavior.training.tier_two_progression",
        "behavior.expansion.pacification_then_claim",
    ]
}

fn load_registry_definitions() -> Result<Vec<IndexedDefinition>, TaxonomyError> {
    let mut files = fs::read_dir(registry_directory())?.collect::<Result<Vec<_>, _>>()?;
    files.sort_by_key(|entry| entry.path());

    let mut indexed = Vec::new();
    for path in files.into_iter().map(|entry| entry.path()) {
        if path.extension().and_then(|extension| extension.to_str()) != Some("yaml") {
            continue;
        }

        let input = fs::read_to_string(&path)?;
        let parsed: RegistryFile = serde_yaml::from_str(&input)?;
        for definition in parsed.entries {
            indexed.push(IndexedDefinition {
                index: indexed.len(),
                definition,
            });
        }
    }

    Ok(indexed)
}

fn validation_context() -> Result<ValidationContext, TaxonomyError> {
    Ok(ValidationContext {
        headings: reference_headings()?,
        implementation_keys: implementation_catalog(),
        exact_coverage_keys: exact_coverage_catalog(),
        behavior_ids: behavior_coverage_ids()
            .iter()
            .map(|id| (*id).to_owned())
            .collect(),
        content_paths: content_field_paths()?,
    })
}

fn validate_registry(
    definitions: &[IndexedDefinition],
    context: &ValidationContext,
) -> Result<(), TaxonomyError> {
    let mut errors = Vec::new();
    let mut ids = BTreeSet::new();
    let children_map = children_map(definitions);
    let definition_map: HashMap<&str, &IndexedDefinition> = definitions
        .iter()
        .map(|definition| (definition.definition.id.as_str(), definition))
        .collect();
    let mut exact_coverage_counts = BTreeMap::<String, usize>::new();
    let mut content_binding_counts = BTreeMap::<String, usize>::new();

    for indexed in definitions {
        let definition = &indexed.definition;
        if !ids.insert(definition.id.clone()) {
            errors.push(format!("duplicate taxonomy id '{}'", definition.id));
        }

        if definition.summary.trim().is_empty() {
            errors.push(format!(
                "taxonomy entry '{}' must have a non-empty summary",
                definition.id
            ));
        }

        if let Some(parent_id) = &definition.parent_id
            && !definition_map.contains_key(parent_id.as_str())
        {
            errors.push(format!(
                "taxonomy entry '{}' references unknown parent '{}'",
                definition.id, parent_id
            ));
        }

        for related_id in &definition.related_ids {
            if !definition_map.contains_key(related_id.as_str()) {
                errors.push(format!(
                    "taxonomy entry '{}' references unknown related id '{}'",
                    definition.id, related_id
                ));
            }
        }

        for section in &definition.reference_sections {
            if !context.headings.contains(section) {
                errors.push(format!(
                    "taxonomy entry '{}' references missing reference heading '{}'",
                    definition.id, section
                ));
            }
        }

        for key in &definition.implementation_keys {
            if !context.implementation_keys.contains(key) {
                errors.push(format!(
                    "taxonomy entry '{}' references unknown implementation key '{}'",
                    definition.id, key
                ));
            }
            if context.exact_coverage_keys.contains(key) {
                *exact_coverage_counts.entry(key.clone()).or_default() += 1;
            }
        }

        for binding in &definition.content_bindings {
            if !context.content_paths.contains(binding) {
                errors.push(format!(
                    "taxonomy entry '{}' references unknown content binding '{}'",
                    definition.id, binding
                ));
            }
            *content_binding_counts.entry(binding.clone()).or_default() += 1;
        }

        for behavior_id in &definition.behavior_ids {
            if !context.behavior_ids.contains(behavior_id) {
                errors.push(format!(
                    "taxonomy entry '{}' references unknown behavior coverage id '{}'",
                    definition.id, behavior_id
                ));
            }
        }
    }

    for key in &context.exact_coverage_keys {
        match exact_coverage_counts.get(key) {
            Some(1) => {}
            Some(count) => errors.push(format!(
                "implementation key '{}' is mapped {} times but must be mapped exactly once",
                key, count
            )),
            None => errors.push(format!(
                "implementation key '{}' is not mapped by any taxonomy entry",
                key
            )),
        }
    }

    for binding in &context.content_paths {
        match content_binding_counts.get(binding) {
            Some(1) => {}
            Some(count) => errors.push(format!(
                "content binding '{}' is mapped {} times but must be mapped exactly once",
                binding, count
            )),
            None => errors.push(format!(
                "content binding '{}' is not mapped by any taxonomy entry",
                binding
            )),
        }
    }

    for indexed in definitions {
        let definition = &indexed.definition;
        let has_children = children_map
            .get(definition.id.as_str())
            .is_some_and(|children| !children.is_empty());
        if has_children {
            continue;
        }

        let Some(status) = definition.status else {
            errors.push(format!(
                "leaf taxonomy entry '{}' must declare a status",
                definition.id
            ));
            continue;
        };

        match status {
            TaxonomyStatus::Implemented | TaxonomyStatus::Partial => {
                if definition.implementation_keys.is_empty() {
                    errors.push(format!(
                        "implemented or partial leaf '{}' must declare implementation keys",
                        definition.id
                    ));
                }
                if definition.kind == TaxonomyEntryKind::Concept
                    && definition.behavior_ids.is_empty()
                {
                    errors.push(format!(
                        "implemented or partial concept '{}' must declare behavior coverage ids",
                        definition.id
                    ));
                }
            }
            TaxonomyStatus::Planned | TaxonomyStatus::Draft => {
                if definition.reference_sections.is_empty() {
                    errors.push(format!(
                        "planned or draft leaf '{}' must declare reference sections",
                        definition.id
                    ));
                }
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(TaxonomyError::Validation(errors))
    }
}

fn children_map(definitions: &[IndexedDefinition]) -> HashMap<&str, Vec<&str>> {
    let mut map: HashMap<&str, Vec<&str>> = HashMap::new();
    for definition in definitions {
        if let Some(parent_id) = definition.definition.parent_id.as_deref() {
            map.entry(parent_id)
                .or_default()
                .push(definition.definition.id.as_str());
        }
    }
    map
}

fn derive_status(
    entry_id: &str,
    definition_map: &HashMap<&str, &IndexedDefinition>,
    children_map: &HashMap<&str, Vec<&str>>,
    cache: &mut HashMap<String, TaxonomyStatus>,
) -> TaxonomyStatus {
    if let Some(status) = cache.get(entry_id) {
        return *status;
    }

    let status = if let Some(children) = children_map.get(entry_id) {
        if children.is_empty() {
            definition_map[entry_id]
                .definition
                .status
                .expect("leaf status validated")
        } else {
            let child_statuses = children
                .iter()
                .map(|child_id| derive_status(child_id, definition_map, children_map, cache))
                .collect::<Vec<_>>();
            combine_statuses(&child_statuses)
        }
    } else {
        definition_map[entry_id]
            .definition
            .status
            .expect("leaf status validated")
    };

    cache.insert(entry_id.to_owned(), status);
    status
}

fn combine_statuses(statuses: &[TaxonomyStatus]) -> TaxonomyStatus {
    if statuses
        .iter()
        .all(|status| *status == TaxonomyStatus::Implemented)
    {
        return TaxonomyStatus::Implemented;
    }

    if statuses.iter().any(|status| {
        matches!(
            status,
            TaxonomyStatus::Implemented | TaxonomyStatus::Partial
        )
    }) {
        return TaxonomyStatus::Partial;
    }

    if statuses.contains(&TaxonomyStatus::Planned) {
        return TaxonomyStatus::Planned;
    }

    TaxonomyStatus::Draft
}

fn domain_for_entry(
    entry_id: &str,
    definition_map: &HashMap<&str, &IndexedDefinition>,
    children_map: &HashMap<&str, Vec<&str>>,
) -> (String, String) {
    let mut current_id = entry_id;
    while let Some(parent_id) = definition_map[current_id].definition.parent_id.as_deref() {
        current_id = parent_id;
    }

    let _ = children_map;
    (
        current_id.to_owned(),
        definition_map[current_id].definition.title.clone(),
    )
}

fn sorted_child_ids(
    entry_id: &str,
    children_map: &HashMap<&str, Vec<&str>>,
    definition_map: &HashMap<&str, &IndexedDefinition>,
) -> Vec<String> {
    let mut children = children_map
        .get(entry_id)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    children.sort_by_key(|child_id| definition_map[child_id.as_str()].index);
    children
}

fn reference_headings() -> Result<BTreeSet<String>, TaxonomyError> {
    let input = fs::read_to_string(reference_path())?;
    Ok(input
        .lines()
        .filter_map(|line| {
            line.strip_prefix("## ")
                .or_else(|| line.strip_prefix("### "))
        })
        .map(|line| line.trim().to_owned())
        .collect())
}

fn implementation_catalog() -> BTreeSet<String> {
    let mut keys = BTreeSet::new();

    keys.extend(CommandDiscriminant::iter().map(CommandDiscriminant::implementation_key));
    keys.extend(EventDiscriminant::iter().map(EventDiscriminant::implementation_key));
    keys.extend(enum_keys::<LocationKind>("location_kind"));
    keys.extend(enum_keys::<InfrastructureKind>("infrastructure_kind"));
    keys.extend(enum_keys::<InfrastructureCondition>(
        "infrastructure_condition",
    ));
    keys.extend(enum_keys::<HostileRemnantKind>("hostile_remnant_kind"));
    keys.extend(enum_keys::<ThreatLevel>("threat_level"));
    keys.extend(enum_keys::<ResourceRichness>("resource_richness"));
    keys.extend(enum_keys::<EnergyPotential>("energy_potential"));
    keys.extend(enum_keys::<BuildCapacity>("build_capacity"));
    keys.extend(enum_keys::<StrategicPosition>("strategic_position"));
    keys.extend(enum_keys::<TerritoryState>("territory_state"));
    keys.extend(enum_keys::<RelayStatus>("relay_status"));
    keys.extend(enum_keys::<LocationVisibility>("location_visibility"));
    keys.extend(enum_keys::<TransitKind>("transit_kind"));
    keys.extend(
        [
            "content.ruleset_document",
            "content.scenario_document",
            "content.world_generation",
            "content.location_connections",
            "core.state.compute_location_economy",
            "core.session.apply_set_throughput_budget",
            "core.session.apply_set_relay_status",
            "core.session.apply_queue_infrastructure_repair",
            "core.session.apply_queue_infrastructure_construction",
            "core.session.apply_dispatch_transit",
            "core.session.apply_survey_location",
            "core.session.apply_start_training_run",
            "core.session.resolve_pacification_arrival",
            "core.session.resolve_claim_arrival",
            "core.session.resolve_assault_arrival",
            "core.session.resolve_strategic_strike_arrival",
        ]
        .into_iter()
        .map(str::to_owned),
    );

    keys
}

fn exact_coverage_catalog() -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    keys.extend(CommandDiscriminant::iter().map(CommandDiscriminant::implementation_key));
    keys.extend(EventDiscriminant::iter().map(EventDiscriminant::implementation_key));
    keys.extend(enum_keys::<LocationKind>("location_kind"));
    keys.extend(enum_keys::<InfrastructureKind>("infrastructure_kind"));
    keys.extend(enum_keys::<InfrastructureCondition>(
        "infrastructure_condition",
    ));
    keys.extend(enum_keys::<HostileRemnantKind>("hostile_remnant_kind"));
    keys.extend(enum_keys::<ThreatLevel>("threat_level"));
    keys.extend(enum_keys::<ResourceRichness>("resource_richness"));
    keys.extend(enum_keys::<EnergyPotential>("energy_potential"));
    keys.extend(enum_keys::<BuildCapacity>("build_capacity"));
    keys.extend(enum_keys::<StrategicPosition>("strategic_position"));
    keys.extend(enum_keys::<TerritoryState>("territory_state"));
    keys.extend(enum_keys::<RelayStatus>("relay_status"));
    keys.extend(enum_keys::<LocationVisibility>("location_visibility"));
    keys.extend(enum_keys::<TransitKind>("transit_kind"));
    keys
}

fn enum_keys<E>(namespace: &str) -> Vec<String>
where
    E: IntoEnumIterator + Into<&'static str>,
{
    E::iter()
        .map(|variant| {
            let variant_name: &'static str = variant.into();
            format!("{namespace}.{variant_name}")
        })
        .collect()
}

fn content_field_paths() -> Result<BTreeSet<String>, TaxonomyError> {
    let ruleset_schema = serde_json::to_value(schemars::schema_for!(RulesetDocument))?;
    let scenario_schema = serde_json::to_value(schemars::schema_for!(ScenarioDocument))?;

    let mut paths = BTreeSet::new();
    collect_schema_leaf_paths("ruleset", &ruleset_schema, &mut paths);
    collect_schema_leaf_paths("scenario", &scenario_schema, &mut paths);
    Ok(paths)
}

fn collect_schema_leaf_paths(prefix: &str, schema: &Value, out: &mut BTreeSet<String>) {
    let defs = schema
        .get("$defs")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    collect_schema_leaf_paths_inner(prefix.to_owned(), schema, &defs, out);
}

fn collect_schema_leaf_paths_inner(
    prefix: String,
    schema: &Value,
    defs: &serde_json::Map<String, Value>,
    out: &mut BTreeSet<String>,
) {
    let Some(resolved) = resolve_schema(schema, defs) else {
        return;
    };

    if let Some(any_of) = resolved.get("anyOf").and_then(Value::as_array) {
        let non_null = any_of
            .iter()
            .filter(|entry| entry.get("type") != Some(&Value::String("null".to_owned())))
            .collect::<Vec<_>>();
        if non_null.is_empty() {
            out.insert(prefix);
            return;
        }

        let complex = non_null.iter().any(|entry| {
            let entry = resolve_schema(entry, defs).unwrap_or(entry);
            entry.get("properties").is_some()
                || entry.get("items").is_some()
                || entry.get("$ref").is_some()
        });
        if complex {
            for entry in non_null {
                collect_schema_leaf_paths_inner(prefix.clone(), entry, defs, out);
            }
        } else {
            out.insert(prefix);
        }
        return;
    }

    if let Some(properties) = resolved.get("properties").and_then(Value::as_object) {
        for (property, property_schema) in properties {
            collect_schema_leaf_paths_inner(
                format!("{prefix}.{property}"),
                property_schema,
                defs,
                out,
            );
        }
        return;
    }

    if let Some(items) = resolved.get("items") {
        collect_schema_leaf_paths_inner(format!("{prefix}[]"), items, defs, out);
        return;
    }

    out.insert(prefix);
}

fn resolve_schema<'a>(
    schema: &'a Value,
    defs: &'a serde_json::Map<String, Value>,
) -> Option<&'a Value> {
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        let definition_name = reference.rsplit('/').next()?;
        return defs.get(definition_name);
    }

    if let Some(inner) = schema.get("schema") {
        return Some(inner);
    }

    Some(schema)
}

fn runtime_value_for_binding(
    binding: &str,
    ruleset_json: &Value,
    scenario_json: &Value,
) -> Option<Value> {
    let mut segments = binding.split('.');
    let root = segments.next()?;
    let path = segments.collect::<Vec<_>>();
    match root {
        "ruleset" => extract_value(ruleset_json, &path),
        "scenario" => extract_value(scenario_json, &path),
        _ => None,
    }
}

fn extract_value(current: &Value, path: &[&str]) -> Option<Value> {
    let Some((segment, rest)) = path.split_first() else {
        return Some(current.clone());
    };

    if let Some(array_segment) = segment.strip_suffix("[]") {
        let array = current.get(array_segment)?.as_array()?;
        return Some(Value::Array(
            array
                .iter()
                .filter_map(|item| extract_value(item, rest))
                .collect(),
        ));
    }

    extract_value(current.get(*segment)?, rest)
}

fn registry_directory() -> PathBuf {
    workspace_root().join("content/taxonomy")
}

fn reference_path() -> PathBuf {
    workspace_root().join("STARFORGE_REFERENCE.md")
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .to_path_buf()
}

fn slugify_markdown_heading(input: &str) -> String {
    let mut slug = String::with_capacity(input.len());
    let mut previous_dash = false;

    for ch in input.chars() {
        let normalized = ch.to_ascii_lowercase();
        if normalized.is_ascii_alphanumeric() {
            slug.push(normalized);
            previous_dash = false;
        } else if !previous_dash {
            slug.push('-');
            previous_dash = true;
        }
    }

    slug.trim_matches('-').to_owned()
}

#[cfg(test)]
mod tests {
    use super::{behavior_coverage_ids, build_taxonomy_document, workspace_root};
    use starforge_content::load_compiled_scenario;
    use starforge_core::{
        BuildCapacity, CommandKind, EnergyPotential, GameSession, HostileRemnantKind,
        HostileRemnantSeed, InfrastructureCondition, InfrastructureKind, InfrastructureSeed,
        LocationConnection, LocationKind, LocationVisibility, MatchSeed, PlayerId, RelayStatus,
        ResourceRichness, ScenarioConfig, SessionId, StartingLocation, StrategicPosition,
        TerritoryState, ThreatLevel,
    };

    #[test]
    fn builds_taxonomy_for_default_repo_content() {
        let document = build_taxonomy_document(default_ruleset_path(), default_scenario_path())
            .expect("taxonomy should build");

        assert_eq!(document.ruleset_name, "starter_skirmish");
        assert_eq!(document.scenario_name, "two_player_skirmish");
        assert!(
            document
                .entries
                .iter()
                .any(|entry| entry.id == "economy.throughput")
        );
        assert!(document.entries.iter().any(|entry| {
            entry.id == "content.ruleset.world_generation"
                && entry
                    .runtime_values
                    .iter()
                    .any(|value| value.binding == "ruleset.world.body_count_min")
        }));
    }

    #[test]
    fn throughput_behavior_is_covered_and_matches_current_economy_math() {
        assert_behavior_id("behavior.economy.throughput_computation");

        let session = default_session();
        let player = session
            .player_view(PlayerId::new(1))
            .expect("player view should load");

        assert_eq!(player.economy.total_connected_energy, 60);
        assert_eq!(player.economy.total_connected_datacenter_capacity, 50);
        assert_eq!(player.economy.usable_throughput, 50);
    }

    #[test]
    fn relay_disconnect_behavior_is_covered_and_zeroes_empire_throughput() {
        assert_behavior_id("behavior.economy.relay_disconnect_zeroes_empire_throughput");

        let mut session = default_session();
        session
            .issue_command_now(
                PlayerId::new(1),
                CommandKind::SetRelayStatus {
                    location_id: 1,
                    relay_status: RelayStatus::Disconnected,
                },
            )
            .expect("relay disconnect should apply");

        let player = session
            .player_view(PlayerId::new(1))
            .expect("player view should load");
        assert_eq!(player.economy.usable_throughput, 0);
        assert_eq!(player.economy.disconnected_owned_location_ids, vec![1]);
    }

    #[test]
    fn repair_behavior_is_covered_and_restores_damaged_infrastructure() {
        assert_behavior_id("behavior.infrastructure.repair_project_completion");

        let mut session = GameSession::new(
            SessionId::new(7),
            starforge_core::GameConfig::default(),
            damaged_datacenter_scenario(),
        );
        session
            .issue_command_now(
                PlayerId::new(1),
                CommandKind::QueueInfrastructureRepair {
                    location_id: 1,
                    infrastructure_kind: InfrastructureKind::Datacenter,
                },
            )
            .expect("repair should queue");
        session.advance_ticks(2);

        let datacenter = session.state().locations[0]
            .infrastructure
            .iter()
            .find(|infrastructure| infrastructure.kind == InfrastructureKind::Datacenter)
            .expect("datacenter should remain present");
        assert_eq!(datacenter.condition, InfrastructureCondition::Operational);
    }

    #[test]
    fn construction_behavior_is_covered_and_adds_new_infrastructure() {
        assert_behavior_id("behavior.infrastructure.construction_project_completion");

        let mut session = GameSession::new(
            SessionId::new(8),
            starforge_core::GameConfig::default(),
            construction_scenario(),
        );
        session
            .issue_command_now(
                PlayerId::new(1),
                CommandKind::QueueInfrastructureConstruction {
                    location_id: 1,
                    infrastructure_kind: InfrastructureKind::Datacenter,
                },
            )
            .expect("construction should queue");
        session.advance_ticks(3);

        let datacenter_count = session.state().locations[0]
            .infrastructure
            .iter()
            .filter(|infrastructure| infrastructure.kind == InfrastructureKind::Datacenter)
            .count();
        assert_eq!(datacenter_count, 2);
    }

    #[test]
    fn survey_behavior_is_covered_and_marks_location_surveyed() {
        assert_behavior_id("behavior.intel.survey_transit_marks_location_surveyed");

        let mut session = GameSession::new(
            SessionId::new(9),
            starforge_core::GameConfig::default(),
            survey_scenario(),
        );
        session
            .issue_command_now(
                PlayerId::new(1),
                CommandKind::DispatchSurveyTransit {
                    origin_location_id: 1,
                    destination_location_id: 2,
                },
            )
            .expect("survey transit should dispatch");
        session.advance_tick();

        let player_view = session
            .player_view(PlayerId::new(1))
            .expect("player view should load");
        let location = player_view
            .locations
            .into_iter()
            .find(|location| location.location_id == 2)
            .expect("survey target should be visible");
        assert_eq!(location.visibility, LocationVisibility::Observed);
        assert_eq!(location.kind, Some(LocationKind::Moon));
        assert!(player_view.visibility.surveyed_location_ids.contains(&2));
    }

    #[test]
    fn training_behavior_is_covered_and_advances_to_tier_two() {
        assert_behavior_id("behavior.training.tier_two_progression");

        let mut session = default_session();
        session
            .issue_command_now(
                PlayerId::new(1),
                CommandKind::SetThroughputBudget {
                    reserved_for_model_upkeep: 0,
                    reserved_for_training: 20,
                    reserved_for_agents: 0,
                },
            )
            .expect("throughput budget should apply");
        session
            .issue_command_now(
                PlayerId::new(1),
                CommandKind::StartTrainingRun { target_tier: 2 },
            )
            .expect("training should start");
        session.advance_ticks(8);

        let player = session
            .player_view(PlayerId::new(1))
            .expect("player view should load");
        assert_eq!(player.model_tier, 2);
        assert!(player.training.is_none());
    }

    #[test]
    fn expansion_behavior_is_covered_and_pacifies_then_claims_neutral_worlds() {
        assert_behavior_id("behavior.expansion.pacification_then_claim");

        let mut session = GameSession::new(
            SessionId::new(10),
            starforge_core::GameConfig::default(),
            hostile_remnant_scenario(),
        );
        session
            .issue_command_now(
                PlayerId::new(1),
                CommandKind::DispatchSurveyTransit {
                    origin_location_id: 1,
                    destination_location_id: 2,
                },
            )
            .expect("survey transit should dispatch");
        session.advance_tick();
        session
            .issue_command_now(
                PlayerId::new(1),
                CommandKind::DispatchPacificationTransit {
                    origin_location_id: 1,
                    destination_location_id: 2,
                },
            )
            .expect("pacification transit should dispatch");
        session.advance_tick();
        session
            .issue_command_now(
                PlayerId::new(1),
                CommandKind::DispatchClaimTransit {
                    origin_location_id: 1,
                    destination_location_id: 2,
                },
            )
            .expect("claim transit should dispatch");
        session.advance_tick();

        let claimed = session.state().locations[1].clone();
        assert_eq!(claimed.controller, Some(PlayerId::new(1)));
        assert_eq!(claimed.territory, TerritoryState::Owned);
        assert!(
            claimed
                .infrastructure
                .iter()
                .any(|infrastructure| infrastructure.kind == InfrastructureKind::CommandNexus)
        );
        assert!(
            claimed
                .infrastructure
                .iter()
                .any(|infrastructure| infrastructure.kind == InfrastructureKind::RelayUplink)
        );
    }

    fn assert_behavior_id(id: &str) {
        assert!(
            behavior_coverage_ids().contains(&id),
            "behavior id '{}' must be registered in the taxonomy coverage catalog",
            id
        );
    }

    fn default_session() -> GameSession {
        let compiled = load_compiled_scenario(default_ruleset_path(), default_scenario_path())
            .expect("default repo scenario should compile");
        GameSession::new(
            SessionId::new(1),
            compiled.game_config,
            compiled.scenario_config,
        )
    }

    fn default_ruleset_path() -> std::path::PathBuf {
        workspace_root().join("content/ruleset.example.yaml")
    }

    fn default_scenario_path() -> std::path::PathBuf {
        workspace_root().join("scenarios/two_player_skirmish.example.yaml")
    }

    fn infrastructure_seed(kind: InfrastructureKind) -> InfrastructureSeed {
        InfrastructureSeed {
            kind,
            tier: 1,
            starts_online: true,
            starts_damaged: false,
        }
    }

    fn compute_homeworld(player_id: PlayerId, location_id: u32, name: &str) -> StartingLocation {
        StartingLocation {
            location_id,
            name: name.to_owned(),
            kind: LocationKind::HabitablePlanet,
            resource_richness: ResourceRichness::Rich,
            energy_potential: EnergyPotential::High,
            build_capacity: BuildCapacity::Expansive,
            strategic_position: StrategicPosition::Balanced,
            territory: TerritoryState::Owned,
            controller: Some(player_id),
            homeworld_of: Some(player_id),
            relay_status: RelayStatus::Connected,
            orbital_slots: 3,
            has_environmental_hazard: false,
            starting_infrastructure: vec![
                infrastructure_seed(InfrastructureKind::CommandNexus),
                infrastructure_seed(InfrastructureKind::MiningSite),
                infrastructure_seed(InfrastructureKind::EnergyProducer),
                infrastructure_seed(InfrastructureKind::Datacenter),
                infrastructure_seed(InfrastructureKind::RelayUplink),
            ],
            hostile_remnant: None,
        }
    }

    fn damaged_datacenter_scenario() -> ScenarioConfig {
        let mut homeworld = compute_homeworld(PlayerId::new(1), 1, "Helios");
        if let Some(datacenter) = homeworld
            .starting_infrastructure
            .iter_mut()
            .find(|seed| seed.kind == InfrastructureKind::Datacenter)
        {
            datacenter.starts_damaged = true;
        }

        ScenarioConfig {
            name: "damaged_datacenter".to_owned(),
            player_ids: vec![PlayerId::new(1)],
            seed: MatchSeed(11),
            starting_locations: vec![homeworld],
            connections: Vec::new(),
        }
    }

    fn construction_scenario() -> ScenarioConfig {
        ScenarioConfig {
            name: "construction".to_owned(),
            player_ids: vec![PlayerId::new(1)],
            seed: MatchSeed(12),
            starting_locations: vec![compute_homeworld(PlayerId::new(1), 1, "Helios")],
            connections: Vec::new(),
        }
    }

    fn survey_scenario() -> ScenarioConfig {
        ScenarioConfig {
            name: "survey".to_owned(),
            player_ids: vec![PlayerId::new(1)],
            seed: MatchSeed(13),
            starting_locations: vec![
                compute_homeworld(PlayerId::new(1), 1, "Helios"),
                StartingLocation {
                    location_id: 2,
                    name: "Survey Target".to_owned(),
                    kind: LocationKind::Moon,
                    resource_richness: ResourceRichness::Moderate,
                    energy_potential: EnergyPotential::Low,
                    build_capacity: BuildCapacity::Constrained,
                    strategic_position: StrategicPosition::Peripheral,
                    territory: TerritoryState::Neutral,
                    controller: None,
                    homeworld_of: None,
                    relay_status: RelayStatus::Disconnected,
                    orbital_slots: 1,
                    has_environmental_hazard: false,
                    starting_infrastructure: Vec::new(),
                    hostile_remnant: None,
                },
            ],
            connections: vec![LocationConnection {
                from_location_id: 1,
                to_location_id: 2,
                travel_time_ticks: 1,
            }],
        }
    }

    fn hostile_remnant_scenario() -> ScenarioConfig {
        ScenarioConfig {
            name: "hostile_remnant".to_owned(),
            player_ids: vec![PlayerId::new(1)],
            seed: MatchSeed(14),
            starting_locations: vec![
                compute_homeworld(PlayerId::new(1), 1, "Helios"),
                StartingLocation {
                    location_id: 2,
                    name: "Frontier".to_owned(),
                    kind: LocationKind::BarrenWorld,
                    resource_richness: ResourceRichness::Moderate,
                    energy_potential: EnergyPotential::Moderate,
                    build_capacity: BuildCapacity::Standard,
                    strategic_position: StrategicPosition::Peripheral,
                    territory: TerritoryState::Neutral,
                    controller: None,
                    homeworld_of: None,
                    relay_status: RelayStatus::Disconnected,
                    orbital_slots: 2,
                    has_environmental_hazard: false,
                    starting_infrastructure: Vec::new(),
                    hostile_remnant: Some(HostileRemnantSeed {
                        kind: HostileRemnantKind::AutonomousDefenseCluster,
                        threat_level: ThreatLevel::Medium,
                        holds_orbital_defenses: true,
                        holds_surface_defenses: true,
                    }),
                },
            ],
            connections: vec![LocationConnection {
                from_location_id: 1,
                to_location_id: 2,
                travel_time_ticks: 1,
            }],
        }
    }
}
