# Starforge Implementation Plan

Status: Active plan; Milestones 1-5 complete, Milestone 6 in progress, Milestone 9 substantially underway, ascension plus conquest, destruction, collapse, interruption, contested-visibility, and HTTP control prototype paths validated  
Last Updated: 2026-03-27  
Source of Truth: [STARFORGE_REFERENCE.md](STARFORGE_REFERENCE.md)  
Current Milestone: API and CLI productization  
Current Focus: Finish the remaining operator ergonomics around the now-functional HTTP control surface, starting with deciding whether CLI metrics, save/load, and scenario-runner flows should land before Milestone 9 is closed.

## Current Execution State

- Current milestone: API and CLI productization
- Current next action: Decide whether to finish Milestone 9 with CLI access to metrics plus persistence or scenario-runner ergonomics, or mark the current HTTP-and-CLI control surface sufficient and return focus to broader simulation depth.
- Blockers: None currently identified.
- Recently completed: Bootstrapped the Rust workspace, added the six planned crates, created baseline tooling with `Makefile`, added placeholder content and scenario files, then completed the simulation-foundation milestone by implementing deterministic pending-command scheduling, persisted replay and pending command data in snapshots, added snapshot restore and replay reconstruction support, added in-memory plus JSON snapshot round-trip tests, replaced the placeholder command path with deterministic throughput and agent-assignment mutations, added seeded RNG state with persistence and regression coverage in `starforge-core`, introduced structured command and domain event payloads, added deterministic location and relay mutations, added a save/load continuation equivalence regression, completed the world-and-content milestone by replacing placeholder YAML parsing in `starforge-content` with typed ruleset and scenario documents, generating deterministic homeworld and neutral starting locations from content data, wiring `GameSession` bootstrap to honor generated starting locations from `ScenarioConfig`, adding file-backed scenario harness loading plus session instantiation in `starforge-scenarios`, adding typed planetary attributes and deterministic location connections so scenarios carry a basic solar-system topology into session state, completed the economy-and-infrastructure milestone by adding runtime infrastructure state, computed energy and datacenter throughput, relay isolation effects, economy-driven throughput budget validation, seeded stockpile initialization, per-location extraction output computation, connected stockpile aggregation, tick-based extraction accumulation, deterministic infrastructure wear thresholds, condition-change events, automatic economy degradation as infrastructure falls into degraded or offline states, repair queues that consume local or connected stockpiles, construction queues for new economic infrastructure that expand throughput and extraction capacity over time, and deterministic duplicate-repair targeting for repeated infrastructure kinds, completed the movement-intel milestone with player-scoped location projections, observed versus stale visibility tracking, route-based survey transit with deterministic ETA and arrival behavior, a player-scoped event feed that exposes only the events a player should currently be allowed to see, generic pacification and claim expeditions, and field-level contested-world major-actions-only projections that now expose relay status plus major-structure events while hiding local queues, stockpiles, and detailed economy data. The file-backed CLI can create sessions, inspect maps and events, step ticks, queue expeditions, manage infrastructure, and drive the prototype loop end to end. The ascension path has been proven in a full playtest from fresh session to tier-five victory. Military-and-victory work now includes assault transits, contested-world state, deterministic takeover resolution, pacification penalties on captured worlds, a first military conquest victory path, restricted contested-world read models, strategic strike destruction with defensive interception, command-collapse countdowns with deterministic single-winner resolution, ascension-site tracking plus interruption on relay or site invalidation, taxonomy coverage for the new victory vocabulary, and CLI-visible collapse or interruption state. API-and-CLI work now includes an in-memory local HTTP/JSON session surface for session creation, summary reads, deterministic stepping, command submission, player-scoped state reads, player-scoped event reads, snapshot save/load, session metrics, a matching API-backed CLI transport activated with `--api-base`, a live CLI-to-server smoke test covering remote session creation and status reads, route-aware map parity through the player-scoped view contract in both file-backed and HTTP-backed CLI modes, and explicit HTTP `run` or `pause` control semantics with matching CLI commands plus regression coverage that proves sessions advance while running and stay fixed once paused.

## Purpose

This document tracks executable implementation progress for the Starforge engine and its supporting tooling. Gameplay design, rules, and product intent live in [STARFORGE_REFERENCE.md](STARFORGE_REFERENCE.md); this file translates that design into buildable engineering milestones, interfaces, acceptance criteria, and resumable execution state.

## Locked Defaults

- Language and build system: Rust workspace using stable Rust unless a specific dependency forces a change.
- Runtime shape: core simulation library plus a local HTTP/JSON API and a thin CLI client.
- First implementation priority: simulation foundation over breadth of features.
- First playable target: headless 2-player skirmish.
- Match control model: API and CLI can drive both empires; no built-in opponent is required for the first playable.
- Inference ownership: engine-managed local inference runtime rather than a separate required model service.
- First concrete inference platform: Apple Silicon.
- Initial content scope: one symmetric ruleset, one solar-system skirmish shape, no UI.
- Determinism rule: accepted commands are authoritative; raw model outputs are never replay inputs.
- Persistence rule: save/load and replay are part of the foundation milestone, not post-MVP polish.

## Architecture Summary

Starforge will be implemented as a Rust workspace with strict ownership boundaries. The simulation is authoritative and deterministic, content is data-driven, inference is treated as a validated command producer, and API or CLI layers are orchestration surfaces rather than alternate sources of game logic.

### `starforge-core`

`starforge-core` owns the authoritative domain model, tick runner, command application, event emission, save/load, replay, and deterministic state transitions. It must not depend on API, CLI, scenario runner, or inference runtime crates. It accepts already-validated configs and content from callers rather than reading files directly.

### `starforge-content`

`starforge-content` owns YAML schemas, content loading, scenario definitions, and validation/conversion into core configuration inputs. It may depend on public config and domain types from `starforge-core`, but `starforge-core` must not depend on `starforge-content`.

### `starforge-inference`

`starforge-inference` owns engine-managed local model lifecycle, prompt assembly, structured intent parsing, timeout handling, and telemetry around accepted or rejected model actions. It may depend on `starforge-core` types for observation snapshots and command construction, but it must never mutate game state directly.

### `starforge-api`

`starforge-api` owns the local HTTP/JSON server, session orchestration, player-scoped read models, and control endpoints for creating, stepping, running, pausing, saving, and loading sessions. It depends on `starforge-core`, `starforge-content`, and later `starforge-inference`.

### `starforge-cli`

`starforge-cli` is a thin operator client over the local API. It should prefer API contracts over direct simulation access so manual use, smoke tests, and scripted runs all exercise the same external control surface.

### `starforge-scenarios`

`starforge-scenarios` owns reusable fixtures, scripted match drivers, and higher-level integration harnesses for deterministic scenario validation. It depends on `starforge-core` and `starforge-content`, and may also drive `starforge-api` for end-to-end tests once the HTTP layer exists.

### Simulation Model

- Authoritative tick: fixed 1-second simulation tick.
- Time model: wall-clock speed is a runner concern; simulation time only advances through ticks.
- Determinism: no wall-clock reads, no unseeded randomness, no floating-point authoritative state math.
- Content format: YAML for rulesets and scenarios.
- Persistence format: versioned JSON for snapshots and replay logs during early development.
- Replay rule: replays persist accepted commands and emitted events, not raw inference outputs.
- Visibility rule: player-facing projections are derived from authoritative state at read time and filtered by ownership, sensors, contest state, and stale intel rules.

## Interfaces and Types to Introduce

### Identity and Configuration Types

| Type | Purpose |
| --- | --- |
| `GameConfig` | Top-level validated ruleset and tunable systems configuration for a match |
| `ScenarioConfig` | Scenario-specific setup including starting state, player slots, and map generation inputs |
| `MatchSeed` | Authoritative seed used for deterministic map generation and seeded simulation randomness |
| `SessionId` | Runtime identity for a live or persisted game session |
| `TickId` | Monotonic simulation tick identifier |
| `PlayerId` | Stable player identity used across commands, state, and visibility filtering |

### Authoritative State Types

| Type | Purpose |
| --- | --- |
| `GameState` | Full authoritative match state at a given tick |
| `PlayerState` | Per-player empire state, including economy, model, visibility, and command collapse state |
| `LocationState` | State for a planetary body or orbital installation |
| `TransitState` | Active movement between locations, including departure, arrival, and visibility implications |
| `VisibilityState` | Player-relative intel state for locations, transit, and events |
| `VictoryState` | Match resolution state, including active, collapse, ascension, and winner resolution |

### Command, Event, and Persistence Types

| Type | Purpose |
| --- | --- |
| `CommandEnvelope` | Validated command wrapper including session, player, and scheduling context |
| `CommandKind` | Typed simulation command variants |
| `ValidationError` | User-facing and developer-facing command rejection reason |
| `EventRecord` | Immutable event emitted by authoritative simulation transitions |
| `ReplayLog` | Ordered accepted command and metadata stream used for deterministic replay |
| `Snapshot` | Versioned serialized representation of session state for save/load |

### Economy, Progression, and Control Types

| Type | Purpose |
| --- | --- |
| `ThroughputBudget` | Allocation and throttling state for model upkeep, agents, research, and training |
| `TrainingRunState` | Status and progress of an active model training run or ascension project |
| `AgentAssignment` | Active agent role, location or scope, reserved throughput, and policy context |
| `RelayStatus` | Connectivity and isolation state for a location relative to the empire network |
| `CommandCollapseState` | Collapse countdown and recovery state for players without an active nexus |

### Local API Surface

| Endpoint | Purpose |
| --- | --- |
| `POST /sessions` | Create a new session from scenario config and seed |
| `GET /sessions/{id}` | Return high-level session metadata and status |
| `POST /sessions/{id}/run` | Start continuous ticking for a session |
| `POST /sessions/{id}/pause` | Pause continuous ticking |
| `POST /sessions/{id}/step` | Advance the simulation by one or more ticks deterministically |
| `POST /sessions/{id}/commands` | Submit one or more player commands |
| `GET /sessions/{id}/state` | Return player-scoped state projection; requires player context |
| `GET /sessions/{id}/events` | Return player-scoped events from a given tick onward |
| `POST /sessions/{id}/save` | Serialize and persist a session snapshot |
| `POST /sessions/load` | Restore a session from a saved snapshot |
| `GET /sessions/{id}/metrics` | Return tick, queue, replay, and inference diagnostics |

### CLI Surface

| Command | Purpose |
| --- | --- |
| `starforge session create` | Create a new session with seed and scenario inputs |
| `starforge session run` | Start continuous execution |
| `starforge session pause` | Pause a running session |
| `starforge session step` | Advance by a fixed number of ticks |
| `starforge session save` | Persist a snapshot |
| `starforge session load` | Restore a saved session |
| `starforge issue` | Submit typed player commands |
| `starforge inspect state` | Read player-scoped state |
| `starforge inspect events` | Read player-scoped events |
| `starforge inspect metrics` | Read simulation and inference diagnostics |
| `starforge scenario run` | Execute scripted deterministic scenario runs |

Current prototype note: before the HTTP API exists, `starforge-cli` is temporarily file-backed and already supports `new`, `status`, `map`, `events`, `step`, `survey`, `pacify`, `claim`, `build`, `repair`, `relay`, `budget`, and `train` directly against snapshot files.

## Milestone Tracker

| Milestone | Status | Goal | Exit Criteria | Dependencies |
| --- | --- | --- | --- | --- |
| Workspace bootstrap | Complete | Create the executable Rust workspace skeleton and baseline developer tooling | Workspace builds, tests run, and crate boundaries compile cleanly | None |
| Simulation foundation | Complete | Establish deterministic ticking, session lifecycle, save/load, and replay | Same seed and command log yield identical state hash and event stream | Workspace bootstrap |
| World and content pipeline | Complete | Load content and generate playable seeded solar systems | Seeded sessions produce valid playable systems with homeworlds and hostile remnants | Workspace bootstrap, Simulation foundation interfaces stable |
| Economy and infrastructure | Complete | Implement extraction, energy, datacenters, maintenance, relays, and throughput | Disconnected worlds and power deficits behave per design | Simulation foundation, World and content pipeline |
| Movement, intel, and control | Complete | Implement transit, survey, fog of war, and player-scoped projections | API visibility differs correctly by player and observation state | Simulation foundation, World and content pipeline, Economy and infrastructure |
| Military and victory | In Progress | Implement combat, conquest, destruction, collapse, and ascension interruption | Scripted matches end by conquest or ascension with no draws | Economy and infrastructure, Movement, intel, and control |
| Progression and agents | In Progress | Implement research, training, model tiers, and non-LLM agent orchestration | Progression and automation pressure the economy correctly | Economy and infrastructure, Movement, intel, and control, Military and victory state contracts |
| Engine-managed local inference | Not Started | Replace placeholder agent policies with real local model-backed intent generation | Model-backed agents produce validated commands without breaking determinism guarantees | Progression and agents |
| API and CLI productization | In Progress | Finalize local control surfaces, metrics, and scenario runner ergonomics | Both empires can be driven end to end through API or CLI | Simulation foundation, Movement, intel, and control, Military and victory, Progression and agents, Engine-managed local inference |
| Hardening and balance harness | Not Started | Build long-run validation and regression tooling | Simulation is stable enough for iterative balancing and regression enforcement | All prior milestones |

## Milestone Details

### 1. Workspace Bootstrap

**Objective**

Stand up a Rust workspace that can support deterministic simulation work without needing to restructure the project later.

**Scope**

- Create the workspace manifest and crate layout.
- Add compileable crate skeletons for the six planned crates.
- Establish lint, format, and test commands.
- Add initial schema placeholder files and directories for content and scenarios.

**Deliverables**

- Root `Cargo.toml` workspace definition
- Crate manifests and starter library or binary targets
- Baseline CI-friendly commands for `fmt`, `clippy`, and `test`
- Placeholder schema modules for config, commands, events, and snapshots

**Acceptance Criteria**

- `cargo check` succeeds for the full workspace.
- `cargo test` succeeds for the full workspace.
- Crate dependency direction matches the architecture summary.

**Risks**

- Over-abstracting too early can slow later iteration.
- Shared utility crates may be added prematurely and create churn.

**Notes for Implementation**

- Prefer stable Rust.
- Keep utility code inside the owning crate until duplication is proven.
- Document dependency rules early to avoid cyclic architecture.

### 2. Simulation Foundation

**Objective**

Build the deterministic simulation kernel and persistence model that every later system will rely on.

**Scope**

- Session lifecycle
- Fixed tick runner
- Seeded RNG
- Command queue and validation pipeline
- Event emission
- Snapshot save/load
- Replay logging and state hashing

**Deliverables**

- Authoritative tick execution loop
- Session creation and destruction path
- Versioned snapshot model
- Replay log writer and replayer
- State hash mechanism for determinism checks

**Acceptance Criteria**

- Replaying the same seed and command log yields identical state hashes and events.
- Save/load followed by replay continuation matches uninterrupted execution.
- Invalid commands are rejected deterministically with structured errors.

**Risks**

- Hidden nondeterminism from time, ordering, or math choices.
- Snapshot versioning drift if introduced too late.

**Notes for Implementation**

- Keep ordering explicit everywhere.
- Use integer or fixed-point authoritative math where possible.
- Treat replay compatibility as a first-class design concern.

### 3. World and Content Pipeline

**Objective**

Convert the design reference into validated data and deterministic world generation.

**Scope**

- YAML content schema
- Scenario definitions
- Planetary body generation
- Homeworld setup
- Orbital slot definitions
- Neutral hazard and hostile remnant support

**Deliverables**

- Content loader and validator
- Scenario schema and sample scenario
- Seeded solar-system generator
- Homeworld placement and initial player setup logic

**Acceptance Criteria**

- A seeded session can generate a valid solar system repeatedly from the same content and seed.
- Each player receives a fair homeworld start under the initial symmetric ruleset.
- Neutral hazards and hostile remnants can be instantiated from content.

**Risks**

- Procedural generation may create unfair or unusable starts.
- Content schema may drift from core assumptions if conversions are implicit.

**Notes for Implementation**

- Keep the first content set intentionally small.
- Fail fast on invalid content instead of silently repairing it.

### 4. Economy and Infrastructure

**Objective**

Implement the resource and capacity systems that make compute a strategic economy.

**Scope**

- Extraction and stockpiled resources
- Construction queues
- Energy production
- Datacenter capacity
- Throughput computation
- Maintenance wear and failure states
- Relay connectivity and isolation effects

**Deliverables**

- Resource accounting model
- Build queue execution model
- Energy and datacenter coupling
- Throughput budget allocator
- Maintenance incident generation and repair handling
- Relay network evaluation

**Acceptance Criteria**

- Power deficits throttle compute and production as designed.
- Disconnected worlds lose access to advanced automation and empire-wide throughput.
- Deferred maintenance can degrade output and create outages.

**Risks**

- Economy rules may become too opaque if too many capacities are coupled at once.
- Maintenance may become noise if incidents are too frequent or too hidden.

**Notes for Implementation**

- Make every penalty visible through state and events.
- Keep resource categories compact and data-driven.

### 5. Movement, Intel, and Control

**Objective**

Implement travel, scouting, fog of war, and player-scoped visibility projections.

**Scope**

- Survey system
- Transit model
- Sensor coverage
- Known, surveyed, observed, tracked, stale, and obscured intel states
- Contested-world visibility
- Player-scoped read-model projection

**Deliverables**

- Transit scheduler and arrival resolution
- Survey actions and reveal rules
- Visibility projection layer for state and events
- Major-actions-only contested visibility behavior

**Acceptance Criteria**

- Two players querying the same session can receive different state views legitimately.
- Contested worlds expose major actions but hide disallowed local details outside direct sensor coverage.
- Stale intel behaves predictably after observation ends.
- The prototype survey -> pacify -> claim loop is playable through CLI control on a seeded skirmish.

**Risks**

- Visibility logic can become inconsistent between state and events.
- Overly broad contested visibility may collapse scouting value.

**Notes for Implementation**

- Centralize visibility filtering instead of scattering it across systems.
- Lock the contested-world projection schema before the API contract is declared stable.
- Current state: survey transit, stale or observed visibility, player-scoped event feeds, pacification expeditions, claim expeditions, and assault-driven contested worlds are implemented; contested attackers now receive a restricted read model that hides queues, stockpiles, and precise economy output, but the remaining gap is refining the full major-actions-only projection described in the reference.

### 6. Military and Victory

**Objective**

Implement military conflict and match resolution end to end.

**Scope**

- Fleets and transports
- Orbital combat
- Surface invasion
- Pacification
- Destruction outcome
- Anti-missile defenses
- Command collapse
- Ascension interruption
- Strict single-winner resolution ordering

**Deliverables**

- Fleet and transport state model
- Combat resolution loop
- Capture and pacification flow
- Strategic warhead and interception flow
- Victory resolver

**Acceptance Criteria**

- Captured worlds transfer damaged infrastructure and remain underproductive during pacification.
- Destroyed worlds stay permanently unusable.
- Matches resolve to a single winner with no draw state.

**Risks**

- Combat math may hide too much if event output is too sparse.
- Conquest could snowball too hard if pacification and damage penalties are weak.

**Notes for Implementation**

- Keep military resolution event-rich for debugging.
- Preserve strict ordering for simultaneous-seeming outcomes.
- Current state: enemy-owned worlds can now be assaulted into contested state, captured through deterministic takeover resolution, pacified back to full productivity, destroyed by costly strategic strikes, defended against those strikes by ground-defense interception, collapsed through loss of active command nexus coverage, and interrupted during tier-five ascension by relay or site invalidation. The main remaining gaps are richer military state beyond the current abstract transit model and the final field-level contested-visibility rules.

### 7. Progression and Agents

**Objective**

Implement technology, training, model tiers, and non-LLM agent behavior sufficient for the first full gameplay loop.

**Scope**

- Research branches
- Four operational model tiers
- Terminal fifth superintelligent end state
- Training runs
- Agent role catalog
- Throughput reservations
- Role-based authority limits

**Deliverables**

- Research progression model
- Training run lifecycle
- Model tier switching
- Placeholder deterministic agent policies
- Throughput reservation and throttling for agents and training

**Acceptance Criteria**

- Training meaningfully competes with automation and research for compute.
- Agents improve scale and responsiveness without bypassing authority boundaries.
- The fifth superintelligent state ends the match immediately.
- A full ascension run can be completed from a fresh session under deterministic CLI control.

**Risks**

- Agents may feel mandatory if baseline manual control is too weak.
- Research and training may overlap too much if boundaries are not clear in code.

**Notes for Implementation**

- Start with deterministic placeholder policies before real model inference.
- Keep authority checks in the command layer, not in individual agent implementations.
- Current state: training tiers 2 through 5, throughput reservation, and the terminal ascension victory path are implemented; research branches and broader agent orchestration are still missing.

### 8. Engine-Managed Local Inference

**Objective**

Integrate a real local model runtime without making the simulation nondeterministic or fragile.

**Scope**

- Apple-Silicon-first runtime adapter
- Model lifecycle and health checks
- Prompt shaping
- Structured intent parsing
- Validation and rejection telemetry
- Timeout and degraded-mode behavior

**Deliverables**

- Inference runtime abstraction
- Concrete Apple Silicon backend
- Observation snapshot formatter
- Structured response parser
- Timeout and fallback policy handling

**Acceptance Criteria**

- Model-backed agents can emit structured intents that become normal validated commands.
- Inference failures do not halt the simulation.
- Replay uses accepted commands only and stays deterministic.

**Risks**

- Runtime integration may dominate effort if scoped too broadly.
- Poorly constrained outputs may create noisy rejection rates.

**Notes for Implementation**

- Keep the first prompt and schema narrow.
- Instrument all rejected or timed-out model actions.

### 9. API and CLI Productization

**Objective**

Turn the engine into a stable local product surface for humans, scripts, and tests.

**Scope**

- Local HTTP/JSON server
- Session control endpoints
- Player-scoped state and event endpoints
- Metrics endpoint
- CLI wrappers
- Scenario runner ergonomics

**Deliverables**

- HTTP server with typed request and response models
- CLI command set matching the planned surface
- Metrics and diagnostics view
- Scenario execution commands

**Acceptance Criteria**

- Both empires can be driven end to end through API or CLI.
- State and event endpoints respect player visibility rules.
- Metrics are sufficient to debug tick, replay, and inference issues.
- Before the HTTP API exists, the temporary file-backed CLI must remain good enough to exercise the playable prototype and validate high-level game flow.

**Risks**

- API contracts may freeze too early if visibility or command models are still moving.
- CLI may drift from API behavior if it bypasses the server.

**Notes for Implementation**

- Keep the CLI thin.
- Prefer schema-tested request and response models.
- Current state: the CLI can now drive the prototype through either file-backed or HTTP-backed transport, and the HTTP API surface covers session creation, run, pause, step, command submission, player-scoped state and events, save/load, metrics, and route-aware map output. The main remaining Milestone 9 question is whether additional operator ergonomics like direct CLI metrics, save/load wrappers, or scenario runner commands are necessary before this milestone is considered complete.

### 10. Hardening and Balance Harness

**Objective**

Make the implementation stable enough to support iterative balance and future UI work.

**Scope**

- Long-run soak tests
- Regression scenarios
- Balance probes
- Replay divergence detection
- End-to-end scenario automation

**Deliverables**

- Automated determinism test suite
- Long-match simulation harness
- Scenario regression set
- Balance probe scripts or harness modules

**Acceptance Criteria**

- Core systems survive long-run simulations without determinism drift.
- Known critical scenarios can be rerun as regression checks.
- The implementation is stable enough for ongoing tuning rather than foundational rework.

**Risks**

- Balance harness can be delayed indefinitely if treated as optional.
- Regression coverage may miss visibility or inference failures if scenarios are too narrow.

**Notes for Implementation**

- Add probes as soon as systems stabilize rather than waiting for full completeness.
- Treat replay divergence as a release-blocking defect.

## Validation Matrix

| Area | Required Scenarios | Gate Milestones |
| --- | --- | --- |
| Determinism | Repeated run of the same seed and command log yields identical state hash and event stream | Simulation foundation, Hardening and balance harness |
| Save/load and replay | Restored sessions continue identically to uninterrupted sessions | Simulation foundation, Hardening and balance harness |
| Economy behavior | Throughput crises from power or datacenter failures throttle correctly; relay cuts isolate worlds and remove advanced automation | Economy and infrastructure, Hardening and balance harness |
| Visibility correctness | Surveyed, observed, stale, and obscured information states project correctly by player; contested worlds now expose relay status, major structures, and major structure completion events while still hiding local queues, stockpiles, and detailed economy timing | Movement, intel, and control; API and CLI productization; Hardening and balance harness |
| Combat and conquest | Hostile remnant expansion can fail if underprepared; captured worlds now transfer damaged infrastructure and recover through pacification; destroyed worlds can now be produced by strategic strike and denied permanently | World and content pipeline, Military and victory, Hardening and balance harness |
| Victory resolution | Ascension victory, relay-cut ascension interruption, deterministic military conquest, strategic-destruction denial, and command-collapse single-winner resolution can all be completed from a fresh session under deterministic control; later military work must still broaden combat state and keep no-draw guarantees intact as the model deepens | Military and victory, Progression and agents, Hardening and balance harness |
| Inference safety | Model-backed agents emit validated commands only; timeouts and invalid outputs degrade safely without halting the sim | Engine-managed local inference, Hardening and balance harness |
| API and CLI control | The file-backed CLI can drive both the ascension prototype and a first conquest-victory prototype headlessly, the local HTTP API now supports session create, run, pause, step, command submission, player-scoped state, player-scoped events, save/load, metrics, and route-aware map reads, and the CLI can now drive those same flows through `--api-base`; the remaining question is whether extra operator ergonomics such as direct CLI metrics, persistence wrappers, or scenario-runner commands are needed before Milestone 9 is closed | API and CLI productization, Hardening and balance harness |

## Open Implementation Questions

- Exact contested-world visibility projection behavior in player-facing read models still needs to be locked at the field level before the state and events API contracts are declared stable.

### Working Default Until Resolved

- Expose major actions during contest: fleet arrivals, orbital battles, landings, command nexus damage or destruction, major structure completion or destruction, pacification start or end, ascension activity, and relay cuts or restores.
- Keep hidden without direct sensors: exact queues, minor repairs, stockpile details, non-engaged hidden forces, and precise production timing.

## Progress Update Protocol

1. When a milestone begins, mark it `In Progress`, update `Current Milestone`, and rewrite `Current Focus` to the concrete slice being executed.
2. When work lands, update `Recently completed` with state-oriented summaries rather than a chronological diary.
3. When blocked, mark the milestone `Blocked` and add the blocker plus the impacted dependency chain.
4. Mark a milestone `Complete` only after its acceptance criteria are satisfied and its validation-matrix scenarios are passing at the intended level.
5. After a coherent slice passes `cargo fmt`, `cargo check`, `cargo test`, and `cargo clippy --workspace --all-targets -- -D warnings`, create a commit before starting the next slice.
6. Keep this file concise and execution-oriented; design rationale belongs in [STARFORGE_REFERENCE.md](STARFORGE_REFERENCE.md), not here.
