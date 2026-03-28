# Starforge Reference

Status: Draft 0.1  
Audience: Internal design alignment  
Scope: Core game design only. Engine, API, CLI, UI, networking, and implementation architecture are intentionally out of scope for this document.

## Document Purpose

This document defines the current reference design for **Starforge**, a real-time strategy game about building an interplanetary industrial empire whose most important strategic resource is constrained AI cognition. It is intended to be the source of truth for game concepts, system relationships, canonical terminology, and unresolved design questions before any code is written.

Starforge is built around one central tension: every unit of industrial and computational capacity can be committed either toward expansion, war, resilience, or self-improvement. A player who overinvests in military can be outscaled by a more efficient rival. A player who overinvests in AI growth can be conquered before reaching superintelligence.

## Canonical Vocabulary

- `Planetary Body`: Any major location on the strategic map, including planets, moons, asteroid clusters, and gas giants.
- `Orbital Installation`: A player-built structure in orbit, such as a relay, shipyard ring, defense lattice, sensor array, or compute platform.
- `Command Nexus`: The structure that establishes sovereignty over a planetary body or orbital installation.
- `Territory State`: The player-relative state of a location: `owned`, `contested`, `neutral`, `destroyed`, or `obscured`.
- `Hostile Remnant`: A localized non-sovereign hostile presence on a neutral world, such as an autonomous defense cluster, rogue colony, or dormant military ruin.
- `Throughput`: Usable AI compute output after datacenter capacity, energy supply, network connection, and infrastructure condition are accounted for.
- `Model Tier`: The current intelligence generation of the player's sovereign model.
- `Training Run`: A long-duration project that consumes throughput and resources to unlock a higher model tier.
- `Agent`: A deployable automation persona that consumes throughput while active and operates inside a defined role.
- `Research Capacity`: The share of infrastructure and throughput committed to technology advancement.
- `Military Production Capacity`: The ability of yards, foundries, and works to create fleets, transports, defenses, and ground forces.
- `Takeover Outcome`: A successful conquest in which control of a location changes hands and surviving production is transferred in damaged form.
- `Destruction Outcome`: A terminal military result in which a planetary body is rendered permanently unusable for future production.
- `Relay Network`: The network that links owned territories into a unified empire economy and compute pool.

## 1. Vision and Pillars

### Premise

Starforge is an RTS set within a single procedurally generated solar system. Each player begins on a homeworld and attempts to dominate the system by expanding industry, coordinating military campaigns, and scaling a local sovereign AI model into a superintelligence before opponents can stop it.

The game is not about directly scripting AI during play. The player deploys and retires predefined agents, allocates scarce throughput, and decides which parts of the empire deserve automation.

### Target Experience

The intended experience is strategic, tense, and legible:

- Expansion should feel powerful but exposed.
- Intelligence should be incomplete enough to make scouting meaningful.
- Compute should feel like a real economic resource, not flavor text.
- Military victory and superintelligence victory should pressure each other throughout the match.
- The player should feel like they are governing an empire, not manually clicking every repair and skirmish.

### Design Pillars

1. `AI as economy`: Compute is a production chain, not a passive upgrade. Datacenters and energy are as strategically important as mines and shipyards.
2. `Automation as tradeoff`: Agents reduce micro and increase efficiency, but they are never free in strategic terms because they consume throughput while active.
3. `Time matters`: Travel, training, repair, and conquest all take time. The game should produce commitments, telegraphed risks, and windows for counterplay.
4. `Imperfect information`: Players can see where bodies are, but they must scout for characteristics and maintain presence to know current activity.
5. `Two valid win paths`: Military conquest and superintelligence must both be credible, interruptible, and readable to opponents.
6. `Strategic permanence`: Captured territory changes the balance of power. Destroyed territory is gone for good.
7. `Generic setting language`: All content uses non-branded, original terminology.

## 2. Player Fantasy and Match Overview

### Player Role

The player is the strategic sovereign of a distributed machine-industrial civilization. They decide where to expand, what to build, how to allocate AI capacity, which worlds to fortify, when to train the next model tier, and whether to capture or annihilate enemy territory.

### What the Player Controls

The player controls:

- Economic development across planets and orbital installations.
- Energy generation and datacenter growth.
- Throughput allocation between automation, research, and training.
- Technology choices and long-term specialization.
- Military production, fleet movement, invasion, and strategic escalation.
- Agent deployment, retirement, and policy boundaries.

### Match Arc

A typical match follows a three-phase arc:

1. `Opening`: Survey nearby worlds, establish early extraction, stabilize homeworld compute, and deploy the first few agents for maintenance and scouting.
2. `Midgame`: Expand the relay network, contest valuable worlds, branch into military or research specialization, and decide whether to begin higher-tier training.
3. `Endgame`: Defend or attack decisive infrastructure, force territorial collapse, or channel enough compute into the superintelligence path to end the match.

### Victory Paths

- `Superintelligence Victory`: Reach the final model tier and complete the final ascension sequence.
- `Conquest Victory`: Eliminate all opposing command sovereignty so no opponent can sustain a functioning empire.

## 3. Core Game Loop

### Primary Loop

The core loop is:

1. Survey and claim new locations.
2. Extract resources and expand industrial infrastructure.
3. Increase energy and datacenter capacity.
4. Convert capacity into usable throughput.
5. Spend throughput on agents, research, and training.
6. Build fleets and defenses to secure the empire.
7. Contest and conquer valuable enemy locations.
8. Concentrate enough strategic advantage to achieve one of the two victory paths.

### Secondary Loops

- `Maintenance loop`: Infrastructure degrades, faults, and must be repaired or replaced.
- `Automation loop`: Agents reduce operational burden, but they compete for throughput with every other compute demand.
- `Progression loop`: Research improves infrastructure and warfare; training improves the model itself.
- `Control loop`: Ownership changes what a player can see, build, and exploit.
- `Recovery loop`: Raids, outages, and failed offensives create instability that must be corrected before growth resumes.

### Core Tensions

- Throughput spent on agents is not available for training.
- Throughput spent on training is not available for agents or research.
- Military expansion without relay and maintenance support becomes brittle.
- Greedy compute scaling creates obvious military targets.
- Conquest is often more profitable than destruction, but destruction may be the correct denial move.

## 4. World and Map Model

### Solar System Structure

Each match takes place in one procedurally generated solar system containing approximately 18 to 24 planetary bodies. Every player begins on a separate homeworld with enough nearby expansion targets to create early branching decisions.

The map includes:

- Habitable planets
- Barren worlds
- Volcanic worlds
- Ice worlds
- Moons
- Asteroid clusters
- Gas giants

Gas giants and asteroid clusters are not conquered through surface occupation, but they can host extraction and orbital infrastructure.

Some neutral worlds are purely empty opportunities, while others contain:

- Passive environmental hazards
- Dormant defense infrastructure
- Active hostile remnants

Hostile remnants are localized obstacles rather than full rival factions. They cannot win the match, expand like players, or establish solar-system sovereignty, but they can defend valuable sites and punish underprepared early expansion.

### What Is Visible at Match Start

At match start, all players can see:

- The star and orbital layout
- The location of every planetary body
- The rough body class of each location

Players cannot initially see:

- Resource profile
- Environmental hazards
- Build potential
- Existing neutral strategic traits
- Current enemy activity outside observed areas

### Survey Rules

Locations must be surveyed before their detailed value is known. Survey can be performed by scout drones, survey fleets, or advanced sensor infrastructure.

Survey reveals:

- Resource deposits
- Development potential
- Environmental risk
- Special strategic traits, if any

Surveyed information remains known permanently unless the location is destroyed.

### Orbital Installations

Each eligible body has a limited set of orbital slots. Orbital installations are critical because they determine whether a location can connect to the empire, build fleets, extend sensor coverage, or host offworld compute.

Core orbital installation families are:

- Relay uplink
- Shipyard ring
- Defense lattice
- Sensor array
- Compute platform
- Harvest platform

### Territory States

`Neutral`
: No player has established sovereignty. The location may be surveyed, exploited lightly, or claimed.

`Owned`
: A single player controls the command nexus and no active opposing claim is underway.

`Contested`
: Two or more players have meaningful military presence or an invasion/claim action is unresolved. Contested locations reveal more information to involved players and operate at reduced efficiency.

`Destroyed`
: The location has been rendered permanently unusable for future production and cannot be claimed again.

`Obscured`
: A player-relative information state indicating that the location exists but current activity is not visible. The underlying territory may still be owned or contested by someone else.

### Relay Network Rule

An owned location contributes fully to the empire only while linked to an operational relay network. A connected location can share resources, research output, and throughput with the wider empire. A disconnected location continues to function locally through standing orders and local production, but it loses access to empire-wide throughput, advanced sovereign-model services, and active agent support until connection is restored.

This makes orbital relays strategic targets and prevents remote expansion from becoming frictionless.

### Travel

All movement between bodies takes time. There is no instant redeployment.

- Travel duration scales with distance and propulsion quality.
- Fleets and transports commit to a route when launched.
- Only departures and arrivals at observed locations are automatically visible.
- Transit is hidden unless an opponent has sensor coverage along the route or a tracked fleet was previously identified.

Travel time is intended to create commitment, feints, and punish overextension.

## 5. Economy Model

### Resource and Capacity Model

Starforge uses a hybrid model of stockpiled resources and live capacities.

#### Stockpiled Resources

- `Common Materials`: Standard construction and repair input used in almost all structures and units.
- `Volatiles`: Fuel-like material used for propulsion, high-output energy systems, and some strategic munitions.
- `Rare Materials`: Advanced industrial input used in higher-tier datacenters, sensors, research, training runs, and strategic weapons.

#### Live Capacities

- `Energy`: Continuous power generation required to operate infrastructure and compute.
- `Datacenter Capacity`: Installed compute infrastructure before energy and condition limits are applied.
- `Throughput`: Usable AI compute available to the player after all constraints.
- `Research Capacity`: Scientific and analytical effort available for technology progression.
- `Military Production Capacity`: Shipyard and foundry bandwidth used to build combat assets.

### Planetary Output

Each owned location has a resource profile and development ceiling. Worlds differ in:

- Extraction richness
- Energy potential
- Environmental wear
- Surface build capacity
- Orbital build capacity
- Strategic position

No world is universally best. Some excel at compute, some at fuel or defense, and some mainly matter because they sit on important routes.

### Development Rule

Infrastructure progresses locally through per-family development queues, one level at a time from level 1 through level 5. Connected locations may draw from empire-wide stockpiles, while disconnected locations can only use their own local stockpiles until the relay network is restored.

This keeps logistics legible without requiring manual cargo routing in the first design.

### Energy and Datacenter Relationship

Datacenters do not generate throughput on their own. Compute becomes usable only when enough energy is allocated to support it.

The governing rule is:

`usable throughput = min(adjusted datacenter capacity, energy-supported compute) x network and condition modifiers`

Where:

- `adjusted datacenter capacity` is installed capacity after technology, condition, and local penalties
- `energy-supported compute` is the amount of compute that can be powered by available energy
- `network and condition modifiers` represent relay status, maintenance faults, and local disruption

This means a player can bottleneck on either power or compute infrastructure.

### Throughput Budget Rule

Throughput is consumed in the following order:

1. Baseline sovereign model upkeep
2. Mandatory empire services and safety systems
3. Player-pinned high-priority projects
4. Active training runs
5. Active agents
6. Research acceleration and optional analysis tasks

If available throughput drops below demand, lower-priority consumers are throttled or suspended first.

### Maintenance and Wear

All critical infrastructure accumulates wear over time. Wear grows faster under:

- High utilization
- Environmental hostility
- Combat damage
- Deferred repairs
- Emergency overdrive

Infrastructure can enter three failure states:

- `Degraded`: Reduced output and rising incident risk
- `Critical`: Severe penalties and imminent outage risk
- `Offline`: No output until repaired

Maintenance requires materials, time, and local labor. Maintenance agents improve detection, prioritization, and response time but do not remove upkeep costs.

### Economic Failure States

Economic crises are intended to be recoverable but painful. Major failure modes are:

- Power deficits that idle compute and production
- Relay cuts that isolate outer worlds
- Deferred maintenance that cascades into outages
- Overbuilt datacenters that cannot be powered
- Aggressive training commitments that starve the operating empire

### Economic Tradeoffs

Players are expected to make meaningful choices between:

- Building wider vs deepening existing worlds
- Power scaling vs datacenter scaling
- Reliable infrastructure vs overdriven burst output
- Automation spending vs manual strategic control
- Defensive hardening vs forward military production

## 6. AI and Agent Systems

### Sovereign Model

Each player possesses a sovereign local model that represents the empire's machine intelligence. In fiction, it is a distributed cognition spanning the player's datacenter network. Mechanically, it is the system that turns throughput into automation, analysis, and eventual superintelligence.

The sovereign model always consumes a baseline amount of throughput simply to remain active. Higher model tiers require more baseline upkeep but provide stronger benefits.

### Model Tiers

The operational progression uses four playable model tiers:

1. `Foundation`: Basic automation and low-level analysis
2. `Analytical`: Better optimization, scouting, and research support
3. `Strategic`: Stronger military coordination, empire planning, and throughput efficiency
4. `Transcendent`: Final pre-victory tier required for the superintelligence endgame

There is also a terminal fifth state:

- `Superintelligent`: The match-ending state reached when a Transcendent empire completes its final ascension project

The fifth state is not a normal operational tier. Reaching it ends the match immediately.

The old operational tier remains available after a higher one is unlocked, but there is little reason to revert except to reduce baseline compute demand in an emergency.

### Training Runs

Training runs are long-duration strategic projects that convert throughput and rare resources into permanent model progression.

Training run rules:

- Only one training run may be active per player at a time.
- A training run consumes reserved throughput for its full duration.
- A training run also consumes materials, especially rare materials, during setup and execution.
- Training cannot be paused. It may only continue or be canceled.
- Canceling a run loses all elapsed time and a meaningful portion of committed resources.
- Completing a standard run unlocks the next operational model tier and allows the player to switch to it.
- The final ascension project pushes a player from `Transcendent` into the terminal fifth state; if it completes, the match ends immediately in that player's favor.

Training is intentionally comparable to advancing an age in a classic RTS: it is a public strategic commitment with opportunity cost.

### Agents

Agents are deployable automation personas that the player activates, assigns, and retires. Agents are not programmed by the player during a match. They are selected from a catalog of role templates.

Deploying an agent has no separate activation cost, but every active agent reserves throughput while it is running.

### Agent Performance Rule

Agent effectiveness depends on three factors:

- Active model tier
- Reserved throughput
- Relevance of the agent's domain to the local task

An under-provisioned agent works slowly, misses optimization opportunities, and responds poorly to bursts of problems. A properly provisioned agent resolves work quickly and reliably. Additional throughput above nominal needs has diminishing returns.

### Agent Limits

There is no hard numeric cap on deployed agents. The real cap is strategic: every new active agent consumes throughput that could have funded training, research, or higher-priority automation.

### Agent Authority Boundaries

Agents may:

- Reorder repairs within their domain
- Optimize production queues inside policy limits
- Respond to maintenance incidents
- Route scouts and logistics assets
- Manage local defensive posture
- Execute standing military doctrines

Agents may not, without explicit player authorization:

- Start or cancel training runs
- Launch strategic weapons
- Scuttle planets or self-destruct infrastructure
- Permanently change empire-wide doctrine
- Trigger ascension victory projects

This boundary keeps automation powerful without making the player feel removed from decisive choices.

### Agent Role Taxonomy

Canonical agent families are:

- `Maintenance Overseer`: Repairs, spare-part allocation, fault triage
- `Economic Optimizer`: Local production priorities, expansion sequencing, infrastructure load balancing
- `Logistics Coordinator`: Relay restoration, transfer readiness, transport staging, reinforcement routing
- `Scout Director`: Survey coverage, route probing, sensor tasking, intel refresh
- `Research Director`: Assigns compute and lab focus to active technology projects
- `Battle Coordinator`: Fleet formations, target priorities, reserve timing, defensive reactions

Additional specialized agents may exist later, but these are the core roles.

### Manual Play Without Agents

The empire remains functional without specialized agents because production queues, movement orders, and basic state progression still operate. What the player loses is optimization, responsiveness, and the ability to manage many simultaneous local problems efficiently.

This is essential: agents increase effective scale, but they must not become mandatory for the game to function at all.

### Disconnected Automation Rule

Disconnected worlds lose access to advanced automation entirely. They may continue executing:

- Existing production queues
- Standing military posture
- Basic local defensive responses
- Manual player-issued orders

They may not benefit from:

- Active specialized agents
- Empire-wide optimization
- Training participation
- Advanced autonomous maintenance and analysis

This makes relay disruption a meaningful military and economic weapon.

## 7. Technology and Progression

### Principle

Technology progression and model progression are separate systems.

- `Technology` improves infrastructure, production, military options, sensors, and resilience.
- `Training` improves the sovereign model itself.

This separation prevents one upgrade track from solving the whole game alone.

### Research Inputs

Research requires:

- Research infrastructure
- Assigned throughput
- Time
- Materials for prototyping and deployment

Research can be accelerated by research-focused agents and higher model tiers, but it still competes for scarce compute.

### Core Technology Branches

- `Power Systems`: Stronger generation, lower compute power cost, better resilience
- `Datacenter Systems`: Higher capacity density, lower wear, improved throughput efficiency
- `Reliability Systems`: Maintenance efficiency, fault prediction, relay hardening
- `Extraction and Industry`: Better yield, improved build speed, stronger offworld production
- `Propulsion and Logistics`: Faster transit, stronger transports, improved remote development
- `Sensors and Intelligence`: Longer detection, better survey speed, stronger fog-of-war penetration
- `Orbital Warfare`: Better fleets, shipyards, and orbital defenses
- `Planetary Warfare`: Stronger invasion forces, surface defenses, and pacification tools
- `Strategic Ordnance`: High-end siege and annihilation options

### Progression Intent

Progression should allow multiple strategic identities:

- Tall compute empire
- Wide industrial empire
- Defensive denial empire
- Fleet projection empire
- Fast ascension empire

The goal is not perfect symmetry of play patterns, but each path must remain answerable.

## 8. Intel, Fog of War, and Information

### Information States

Each location can be in one of several information states for a given player:

- `Known`: The body exists and its orbit is visible
- `Surveyed`: Its properties are understood
- `Observed`: Current activity is visible because the player has direct sensor coverage or control
- `Tracked`: A moving force is currently being followed
- `Stale`: The player has older information from a prior observation
- `Obscured`: Current details are unknown

### Visibility by Control State

- `Owned`: Full live visibility of infrastructure, queues, fleets, incidents, and local resources
- `Contested`: Live visibility of major combat actions and local force presence; partial uncertainty remains outside active coverage
- `Neutral`: Only surveyed traits are known unless current sensors are present
- `Enemy controlled`: Only stale or current sensor-based information is known
- `Destroyed`: The state is globally visible once confirmed

### Sensor Sources

Primary intel sources are:

- Scout drones
- Patrol fleets
- Orbital sensor arrays
- Contested ground presence
- Advanced detection technologies

### Information Design Intent

Intel is meant to reward initiative, not pure clicking. The player should be able to create meaningful uncertainty:

- Hide buildup on remote worlds
- Feint with transit routes
- Bluff toward military escalation while pursuing training
- Raid relays to blind or isolate an opponent

At the same time, the system must remain readable enough that losses feel earned, not arbitrary.

## 9. Military and Conflict

### Military Principle

Military action is the enforcement arm of strategy. Fleets are not independent mini-games; they exist to create or deny sovereignty, defend strategic infrastructure, and interrupt the opponent's compute economy or victory timing.

### Unit Families

Core military families are:

- `Scout Drones`: Fast recon and route visibility
- `Escort Craft`: Fast protection against scouts, raiders, and transports
- `Line Warships`: Main fleet combatants for orbital control
- `Siege Vessels`: High-cost ships for breaking defenses and threatening hardened targets
- `Transport Ships`: Move invasion forces, expansion packages, and support payloads
- `Ground Forces`: Required for surface capture
- `Air Defense Wings`: Planetary defensive air or aerospace cover
- `Ground Defense Sites`: Surface fortifications and anti-landing systems
- `Strategic Warheads`: Late-game denial and destruction tools

### Military Production

Military assets require the appropriate production structures:

- Orbital shipyards for fleets
- Surface military works for ground forces
- Ordnance complexes for strategic weapons
- Defense construction yards for static fortifications

Military production also competes for materials, energy, and maintenance attention.

### Conflict Layers

Conflict resolves across three layers:

1. `Transit and approach`: Forces commit to movement and reveal intent over time
2. `Orbital control`: Fleets and orbital defenses fight for supremacy around the target
3. `Surface invasion`: Ground forces and planetary defenses determine takeover if the attacker seeks conquest

An attacker cannot capture a developed planet through orbital presence alone. Surface control is required.

### Conquest Rules

To conquer a developed planetary body, the attacker must:

1. Reach the target with a fleet and survive transit
2. Defeat or suppress defending orbital forces
3. Land invasion forces
4. Break the defender's command nexus and planetary defense network
5. Hold the location long enough to complete pacification

Pacification is a short stabilization period during which the world remains vulnerable and underproductive.

### Takeover Outcome

When a location is captured:

- Command sovereignty changes hands
- Surviving structures transfer in damaged condition
- Production is reduced until pacification ends
- Local defenses may remain partially disabled
- The new owner gains access to the location's future economic and military output

Capture is usually the more efficient military outcome because the winner preserves productive value.

### Destruction Outcome

Destruction is a separate and more extreme result. A destroyed planetary body:

- Cannot be claimed again
- Cannot host future production
- No longer contributes resources, compute, or military value
- Remains on the map as a dead location

Planetary destruction requires dedicated strategic escalation. Standard fleet victory does not automatically destroy worlds.

### Why Destroy Instead of Capture

Destruction is appropriate when:

- A world is too fortified to capture before an opposing victory timing
- The attacker wants to deny rare deposits or compute concentration
- The defender is using the world for ascension infrastructure
- The attacker cannot hold the location even if they win the battle

### Strategic Warhead Rules

Strategic warheads do not require a separate escalation track beyond their normal prerequisites. They are gated by:

- High research investment
- Significant material cost
- Specialized production infrastructure
- Long build lead time

They are intended to be rare and decisive, not routine.

Strategic warheads can be countered by dedicated anti-missile defenses on:

- Orbital defense infrastructure
- Planetary defense infrastructure
- Specialized escort and interception screens

This keeps strategic weapons threatening without making them unconditional.

### Military Technology Investment

Military technology increases:

- Fleet efficiency
- Invasion quality
- Defensive resilience
- Strategic range
- Siege capability
- Weapon lethality

This gives military-focused empires a genuine path to overpower a more compute-focused rival before that rival completes ascension.

## 10. Victory, Defeat, and Match End Conditions

### Superintelligence Victory

The superintelligence path culminates in the `Ascension Sequence`.

To begin the ascension sequence, a player must:

1. Unlock the `Transcendent` model tier
2. Control at least one connected `Ascension Complex`, a late-game compute installation built on a developed world or orbital installation
3. Meet the required sustained throughput and energy threshold
4. Avoid active contest on the ascension site while the sequence is channeling

If the sequence completes uninterrupted, the player's sovereign model crosses into the terminal fifth `Superintelligent` state and that player wins immediately.

### How Opponents Counter Ascension

Ascension can be interrupted by:

- Destroying or capturing the ascension site
- Forcing the player's throughput below the required threshold
- Cutting relays that isolate the site from the empire network
- Triggering critical infrastructure failures
- Opening active contest on the site

This keeps the superintelligence path visible and interactive instead of solitary.

### Conquest Victory

A player achieves conquest victory when all opponents are eliminated through command collapse.

### Command Collapse Rule

Each empire requires at least one active command nexus to remain sovereign. A player enters `collapse` when they control no command nexus anywhere in the system.

While collapsing:

- Existing forces may still fight
- Remaining infrastructure may still function briefly
- The player may recover by establishing or recapturing a command nexus before the collapse countdown expires

If the countdown expires with no command nexus restored, the player is defeated and removed from victory contention.

### Defeat State

Defeated players lose sovereignty. Their remaining assets are treated as broken, abandoned, or self-terminating and no longer count as active factions in the match.

### Edge Cases

There are no draw states in standard Starforge play.

- Combat, collapse, and ascension resolve in strict simulation order.
- If two empires appear to destroy each other at nearly the same moment, the empire whose defeat condition resolves first loses first.
- If two ascension sequences would nominally complete in the same final resolution window, the one that resolves first wins immediately and ends the match.
- If a player's ascension sequence completes before their collapse countdown expires, ascension victory takes precedence.

### Comeback Paths

Comebacks are expected to exist through:

- Hidden expansion into underobserved territory
- Relay restoration after a raid
- Desperate reconquest of a nexus world
- Targeted strikes on a fragile ascension setup
- Capturing enemy infrastructure instead of merely surviving

## 11. Match Flow Walkthrough

### Opening Phase

The opening is about information and stable foundations.

- The player starts with a homeworld containing a command nexus, basic extraction, initial power generation, an early datacenter, light defenses, and enough production to launch scouts and early expansion units.
- The first meaningful decisions are where to survey, whether to prioritize local compute or additional extraction, and whether to deploy early maintenance or scouting agents.
- Nearby worlds are partially known by location only, so expansion carries risk.

Expected opening tension: a player who races compute may expose themselves militarily, while a player who races military may never reach a sustainable throughput lead.

### Midgame

The midgame begins once multiple worlds are connected and first major fleet actions become plausible.

- Relay network shape starts to matter.
- Outer worlds become attractive but harder to defend.
- Maintenance burden grows.
- Research branching becomes visible in playstyle.
- One player may start a major training run, signaling long-term intent.
- Raids against relays, shipyards, and compute hubs become decisive.

Expected midgame tension: players can no longer do everything. The empire has enough scale that unautomated weaknesses start to punish them.

### Endgame

The endgame revolves around concentrated strategic commitments.

- One player may attempt ascension.
- One player may commit to conquest before ascension becomes unstoppable.
- Strategic weapons and siege platforms enter the decision space.
- Capturing or destroying a single key world can decide the match.

Expected endgame tension: both victory paths become visible, vulnerable, and urgent.

### Design Review Scenarios

The following scenarios should remain valid whenever the document is revised:

- `Opening expansion`: A player must choose between early compute growth, surveying, and fast territorial claim.
- `Throughput crisis`: A power shortfall or datacenter outage forces the player to cut agents, delay research, or cancel military ambitions.
- `Training gamble`: A player begins a higher-tier training run and becomes temporarily easier to raid or outproduce.
- `Hidden enemy growth`: An opponent uses obscured space to prepare fleets or compute infrastructure away from direct observation.
- `Planet invasion`: A target moves from approach to orbital battle to surface capture, with takeover and destruction both remaining possible.
- `Captured production swing`: A successful conquest shifts the economic balance but does not instantly hand the attacker a pristine world.
- `Endgame divergence`: One player commits to ascension while the other commits to military interruption or command collapse.

## 12. Content Taxonomy

### Resource Types

| Type | Role |
| --- | --- |
| Common Materials | Core construction, repairs, standard units, generic infrastructure |
| Volatiles | Fuel-intensive propulsion, advanced power systems, select munitions |
| Rare Materials | Higher-tier compute, sensors, training, strategic systems |
| Energy | Live operational power; required for compute and infrastructure |
| Throughput | Usable sovereign AI compute budget |
| Research Capacity | Technology advancement bandwidth |
| Military Production Capacity | Unit and defense manufacturing bandwidth |

### Infrastructure Families

| Family | Purpose |
| --- | --- |
| Command Nexus | Establishes sovereignty and prevents command collapse |
| Mining Site | Extracts local stockpiled resources |
| Refinery Complex | Improves extraction output and processing efficiency |
| Energy Producer | Generates power for industry and compute |
| Datacenter | Provides compute capacity |
| Relay Uplink | Connects a location to the empire network |
| Shipyard Ring | Produces fleets and orbital logistics assets |
| Military Works | Produces ground forces and defense equipment |
| Defense Lattice | Provides orbital protection and anti-missile interception |
| Ground Defense Site | Provides anti-landing, local defense strength, and missile interception |
| Sensor Array | Extends survey and detection coverage |
| Compute Platform | Provides offworld compute in orbit |
| Ascension Complex | Hosts the final superintelligence victory project |

Each infrastructure family on a world follows a single progression path rather than allowing arbitrary duplicate build entries in the player-facing model.

### Unit Families

| Family | Role |
| --- | --- |
| Scout Drone | Reconnaissance, survey, route monitoring |
| Escort Craft | Fast anti-scout and anti-transport defense |
| Line Warship | Main orbital battle unit |
| Siege Vessel | Anti-defense and hardened-target specialist |
| Transport Ship | Moves ground forces and expansion packages |
| Ground Force | Captures and defends planetary surfaces |
| Air Defense Wing | Planetary anti-air and aerospace protection |
| Strategic Warhead | Costly denial and destruction tool that can be intercepted by dedicated defenses |

### Agent Roles

| Role | Primary Function |
| --- | --- |
| Maintenance Overseer | Repairs and fault triage |
| Economic Optimizer | Production sequencing and infrastructure balancing |
| Logistics Coordinator | Remote support, route readiness, relay response |
| Scout Director | Intel coverage and survey planning |
| Research Director | Technology focus and compute assignment |
| Battle Coordinator | Fleet doctrine and tactical automation |

### Planetary Attributes

| Attribute | Meaning |
| --- | --- |
| Resource Richness | Quantity and quality of extractable stockpiled resources |
| Energy Potential | Suitability for high-output power generation |
| Build Capacity | Number and scale of viable surface installations |
| Orbital Capacity | Number of useful orbital installation slots |
| Environmental Hazard | Ongoing wear, disruption, or defensive challenge |
| Strategic Position | Route control, map centrality, or staging value |

### Technology Branches

| Branch | Focus |
| --- | --- |
| Power Systems | Energy scale and efficiency |
| Datacenter Systems | Compute density and output |
| Reliability Systems | Wear control, repair speed, relay hardening |
| Extraction and Industry | Resource yield and build speed |
| Propulsion and Logistics | Travel speed and remote force projection |
| Sensors and Intelligence | Detection, survey, and information quality |
| Orbital Warfare | Fleet and orbital combat power |
| Planetary Warfare | Invasion and defensive ground power |
| Strategic Ordnance | High-end denial, destruction capability, and missile defense countermeasures |

## 13. Balancing Framework

### Non-Negotiable Balance Goals

- Early-game throughput must be scarce enough that the player cannot fully automate everything.
- Expansion must usually increase long-term strength, but it must create defendable vulnerabilities.
- Capture should be the default profitable military outcome; destruction should be situational, not routine.
- The ascension path must be faster for focused players but vulnerable enough that military pressure can still stop it.
- Military-focused players must be able to win before a compute player becomes inevitable.
- Information denial should create uncertainty, not complete opacity.

### Tuning Principles

- Higher-tier infrastructure should be more efficient but more maintenance-sensitive.
- Wide empires should gain raw output but pay more in defense and network exposure.
- Tall empires should gain concentrated compute but present obvious raid targets.
- Agents should meaningfully increase operational scale, but never remove the underlying economic cost of what they manage.
- Strategic weapons should feel decisive, expensive, and reputationally extreme within the fiction.

### Values Intentionally Left Open for Later Tuning

- Exact yields by body type
- Exact travel times
- Exact training durations
- Exact baseline throughput upkeep per model tier
- Exact combat math
- Exact number of orbital slots and build slots
- Exact collapse countdown duration
- Exact ascension channel duration and threshold

The mechanics are locked in principle; the numbers are not.

## 14. Design Risks and Failure Modes

### Snowballing From Conquest

Risk: capturing enemy worlds could create unstoppable positive feedback.

Mitigations:

- Captured worlds transfer damaged, not pristine
- Pacification delays full output
- Relay and maintenance burden increase with empire size
- Outer expansion creates more attack surface

### Runaway Agent Automation

Risk: the optimal play becomes spawning agents for everything and removing meaningful decision-making.

Mitigations:

- Agents consume live throughput
- Agents only improve work inside role boundaries
- Agents cannot make irreversible strategic choices without approval
- Higher model tiers still face competing demands from training and research

### Opaque AI Behavior

Risk: players stop understanding why things happen because too much is delegated.

Mitigations:

- Agent roles are narrow and named
- Agent authority boundaries are explicit
- Critical actions remain manual by rule
- Failure and throttling are explained through visible system states

### Excessive Economic Complexity

Risk: the game becomes a spreadsheet about remote infrastructure rather than an RTS.

Mitigations:

- Keep the stockpiled resource set compact
- Use connected empire pools instead of fully manual shipping
- Make relay connection the main logistics abstraction
- Let agents handle local optimization rather than forcing repetitive micromanagement

### Stalled Endgames

Risk: players turtle, deny each other, and fail to close matches.

Mitigations:

- Ascension creates a forced-answer win condition
- Command collapse prevents irrelevant remnant survival
- Destruction exists as a high-cost denial option
- Key infrastructure concentration creates real decisive targets

## 15. Decisions Log and Deferred Questions

### Open Questions

- How much hidden information should remain on contested worlds outside direct sensor coverage?

### Resolved Decisions

- Starforge is a real-time strategy game set in one solar system.
- The player's core differentiator is a throughput-limited sovereign AI model.
- Players deploy agent templates during a match; they do not program agents during play.
- The strategic map shows all planetary body locations at the start, but not detailed characteristics.
- There are two primary victory paths: superintelligence and conquest.
- Throughput depends on both datacenter capacity and available energy.
- Relay connectivity determines whether remote locations contribute fully to empire-wide production and compute.
- Travel across the map always takes time.
- Capture and destruction are distinct military outcomes.
- Destroyed worlds are permanently removed from future production.
- Standard matches do not allow draws; the simulation always resolves to a single winner.
- Some neutral worlds may contain hazards or localized hostile remnants, but not full sovereign rivals.
- Starforge uses four playable model tiers; entering the fifth state ends the match immediately.
- Strategic warheads are gated by cost, tech, and production complexity rather than a separate escalation system.
- Dedicated anti-missile defenses can counter strategic warheads.
- Disconnected worlds lose access to advanced automation until relay connection is restored.
- Command nexus structures use a universal ruleset across controlled locations.
- The first full design artifact is one internal master reference document.

### Assumptions

- The baseline balance target is a competitive two-player match, even if the design may later support more players.
- Neutral locations do not begin with major sovereign factions, though some may contain hazards or limited hostile remnants.
- Resource shipping is abstracted through connected empire pools rather than modeled as manual cargo logistics.
- Orbital installations are as strategically important as surface holdings.
- The player always has some minimal ability to operate manually even without specialized agents.

### Deferred Topics

- Campaign structure and narrative framing
- Diplomacy and alliances
- Modding support
- Multiplayer networking model
- UI and player input model
- Save/load and replay rules
- Engine architecture and data schema
- AI implementation details for the local model runtime

## Closing Statement

Starforge is designed around a simple but strong strategic proposition: build the industrial and computational base of a solar-system empire faster than your opponent can disrupt it, then convert that advantage into either overwhelming conquest or irreversible superintelligence. Every major system in this document exists to reinforce that proposition.
