# Calibrator V2 Design Spec

## Status

This document defines the target architecture for a full rewrite of the Janiform calibrator core.

It is not a migration note and it is not a compromise plan. The intent is to specify the
calibrator we actually want, using what v1 taught us, without preserving weak assumptions just
because they already exist in code.

The model family is assumed to be capable of representing a broad set of financial assets. The
problem to solve here is therefore calibration architecture, not model expressiveness.

## Purpose

Build a calibrator that:

- works across materially different asset classes, not only EUR/USD
- remains computationally practical on large datasets, including roughly year-scale datasets with
  tens of millions of rows
- uses the available CPU budget intelligently
- avoids arbitrary search boxes, arbitrary horizon blends, and arbitrary objective weights
- produces rich, structured diagnostics that make every calibration run inspectable and debuggable
- supports both human inspection and machine inspection by downstream tools and LLMs

## Non-Negotiable Design Rules

1. No knowingly temporary heuristic placeholders.
   If a component can be specified correctly now, it must be specified correctly now.

2. No hidden magic numbers.
   Any constant that survives into the design must be one of:
   - mathematically implied
   - statistically estimated from observed data
   - an explicit policy parameter with a written rationale and dedicated diagnostics

3. No baseline-centered local search boxes for important parameter blocks.
   Search geometry must be data-adaptive or model-adaptive, not tied to the current staged guess by
   arbitrary multipliers.

4. No single giant weighted-sum objective as the primary decision mechanism.
   Calibration is a constrained multi-objective problem and should be treated as such.

5. Debugging and evaluation are part of the calibrator, not optional tooling around it.
   Every calibration run must produce structured artifacts that explain what the solver saw, what it
   tried, what it rejected, and why it selected the final candidate.

6. Determinism is required.
   Repeated runs with the same data, seeds, and configuration must produce the same decisions and
   diagnostics.

7. No monolith modules.
   The calibrator core and the workbench must be split into focused modules with clear
   responsibilities. A single giant file that mixes targets, solvers, rendering, and interaction is
   unacceptable.

8. Shared helpers belong in shared locations.
   Any helper that is general enough to be reused must live in a shared module or existing shared
   crate such as `janiform-foundations`, not as a local helper copy inside one solver or GUI file.
   If an appropriate helper already exists, it must be reused or moved to a proper shared location.
   It must never be copied into a second implementation. Duplicate helpers are forbidden.

## Lessons From V1

V1 established several important facts:

- staged preprocessing is useful
- seasonality freezing is a good idea
- simulation-backed joint refinement is necessary
- multihorizon evaluation matters
- broad deterministic candidate evaluation parallelizes well

V1 also exposed major architectural problems:

- too much logic is embedded in heuristic score composition
- important search regions are constrained by arbitrary local boxes
- a single optimizer family is used on subproblems with very different geometry
- too much weight is placed on scalarized summary scores
- some diagnostics reward the right shape with the wrong level
- debugging output is not yet rich enough to explain solver failures unambiguously

The v2 design must preserve the first group and remove the second.

## Goals

### Statistical goals

- fit a realistic deterministic seasonality surface
- fit Hawkes and activity dynamics with correct level, shape, and clustering behavior
- fit the continuous return law with correct scale, center mass, asymmetry, and tails
- fit event-conditioned jump and news behavior without collapsing important distinctions
- reconcile these layers under a joint objective grounded in the model’s generative semantics

### Engineering goals

- scale to large datasets without pathological runtime
- make good use of high-core-count machines
- separate target construction, geometry construction, solving, evaluation, and selection
- make every calibration run reproducible, inspectable, and explainable
- provide a workbench UX that keeps the user continuously informed during long-running operations

### Product goals

- one calibrator architecture for a broad panel of assets
- no EUR/USD-specific hidden assumptions
- enough diagnostic depth to make failures actionable instead of mysterious

## Non-Goals

- preserving v1 internals
- preserving v1 objective structure
- preserving v1 optimizer choices
- forcing all subproblems through one common optimizer API if that damages fit quality
- treating path-specific extrema as primary fit targets when they are not statistically stable

## Core Architecture

Calibrator v2 is organized as five explicit layers:

1. `ObservedTargetBuilder`
2. `ParameterGeometryBuilder`
3. `BlockSolvers`
4. `EvaluationEngine`
5. `SelectionAndDiagnosticsEngine`

These layers must be physically separated in the codebase. Target construction, solver logic, and
diagnostic rendering must not be entangled in one module.

## 1. Observed Target Builder

### Purpose

Construct a canonical, typed representation of the empirical targets implied by the training data.

This layer is the source of truth for what the calibrator is trying to match. All downstream
solvers and evaluators consume this object rather than recomputing ad hoc targets.

### Required target families

#### Activity targets

- open-session total event count
- per-hour event counts
- hourly profile shape
- hourly profile level
- intra-hour and inter-hour clustering statistics
- burstiness and overdispersion diagnostics
- close-to-open transition activity diagnostics

#### Return targets

- block return distributions for a configurable horizon family
- central mass diagnostics
- scale diagnostics
- asymmetry diagnostics
- tail diagnostics
- business-time-conditioned return diagnostics
- conditional diagnostics stratified by activity regime where identifiable

#### Coupling targets

- realized-variance versus activity co-movement
- lagged activity versus return-volatility coupling
- event-conditioned volatility response
- post-shock activity response

#### Event-conditioned targets

- known-news response diagnostics
- inferred surprise-event diagnostics
- jump mark conditional diagnostics
- open-jump versus closed-gap diagnostics

### Required metadata per target

Every target entry must carry:

- definition metadata
- estimation method metadata
- effective sample size
- uncertainty or stability metadata
- evaluation-fidelity eligibility metadata
- admissibility role
- ranking role
- horizon or conditioning metadata

This avoids pretending that all metrics are equally reliable or equally important.

Evaluation-fidelity eligibility metadata must classify whether a target is:

- exact-eligible
- coarse-eligible
- tick-required

This classification is part of the target contract and drives solver budget allocation.

### No direct use of raw training min and max

Path-extreme quantities such as raw realized price span are too path-specific to be used as primary
fit targets. The observed training span is one realization, not a stable population quantity.

Therefore:

- raw training min and max must not be used as a core objective target
- a single realized price span must not be treated as a hard realism constraint
- path-extreme functionals may still appear as diagnostic or auxiliary admissibility evidence, but
  only through statistically framed summaries, not naive equality to the training sample

Examples of acceptable path-extreme use:

- distribution of span over rolling windows
- exceedance-rate diagnostics relative to observed horizon-matched windows
- quantile-based path-functional comparison over many windows

Examples of unacceptable use:

- forcing the simulated full-sample span to match the observed full-sample span
- using one realized max-minus-min as a central fit target

## 2. Parameter Geometry Builder

### Purpose

Define how each model block is explored.

This layer translates observed targets into parameter coordinates, candidate families, admissible
regions, and local neighborhoods. It must be data-adaptive and model-aware.

### Design rule

Search geometry must be derived from:

- mathematical constraints
- identifiability structure
- observed target summaries
- profile diagnostics

It must not be derived from:

- fixed multipliers around a staged guess
- arbitrary log-space half-spans
- hand-tuned asset-specific boxes

### Required geometry families

#### Hawkes geometry

Use coordinates tied to:

- baseline intensity level
- branching structure
- decay scales
- asymmetry structure if present
- seasonality-adjusted stability margin

Search regions should be framed in terms of:

- valid stability domain
- identifiable activity-level bands from observed counts
- event clustering diagnostics

#### NIG and continuous-law geometry

This is the most geometry-sensitive block and must not be treated like a generic local box search.

Search coordinates should be built from quantities closer to observed distribution shape, such as:

- scale
- central mass
- asymmetry
- tail balance
- side-specific exposure effects

The builder must use:

- analytical validity constraints
- observed quantiles and moment-like summaries
- profile diagnostics derived from exact block samples
- multiple candidate families spanning distinct plausible regimes

The geometry must support:

- broad deterministic candidate generation
- locally refined neighborhoods around elite candidates
- coupled movement of strongly interacting parameters such as `alpha` and `delta`

#### News and coupling geometry

Use coordinates tied to:

- event-conditioned activity response
- event-conditioned jump contribution
- calendar- or state-conditioned arrival behavior

### Geometry outputs

Each geometry builder must return:

- parameter coordinate definitions
- admissible-domain checks
- deterministic candidate generators
- local neighborhood generators
- symmetry or invariance notes
- diagnostics that explain why a region is plausible or implausible

## 3. Solver Architecture

### Design rule

There is no single best optimizer for every block. V2 must use different solver styles for
different subproblems.

### 3.1 Seasonality solver

Keep the core v1 idea:

- fit deterministic calendar structure from timestamps only
- enforce mean-one normalization over the calibration window
- freeze the materialized seasonality representation for downstream use

This stage should be exact or near-exact and should not consume unnecessary simulation budget.

### 3.2 Hawkes solver

The Hawkes solver should not be a candidate table plus arbitrary scalar score.

It should use:

- data-adaptive candidate generation
- stability-aware admissibility checks
- exact or cheap deterministic activity diagnostics
- deterministic parallel search over candidate families
- local refinement within admissible activity geometry

The Hawkes solver must explicitly optimize for:

- level
- shape
- clustering
- stability margin

### 3.3 Continuous-law solver

The continuous-law solver must be a first-class specialized solver. It should not be delegated to a
generic line-search method over raw transformed parameters.

#### Required solver shape

1. deterministic broad candidate generation from data-adaptive geometry
2. exact block-level scoring on observed aggregated samples
3. elite set retention
4. deterministic local refinement around elites
5. return of a ranked elite frontier, not only one point

#### Local refinement algorithm

Powell must not be the default assumption here.

The preferred design is:

- deterministic pattern or poll search over coupled local neighborhoods
- adaptive neighborhood radius updates
- explicit coupled directions for known interacting coordinates
- batched evaluation of neighborhood proposals in parallel

The algorithm must be chosen for:

- rough but deterministic objective surfaces
- coupled parameters
- narrow ridges
- multimodality risk

### 3.4 Event and coupling solvers

Known-news response, surprise-event behavior, mark-law fitting, and activity coupling should be
solved as explicit subproblems with their own target families and admissibility logic.

They may use:

- exact fitting where available
- deterministic candidate evaluation
- local refinement where justified

They must not be buried inside one opaque catch-all scoring file.

### 3.5 Joint reconciliation solver

Joint refinement in v2 is not a monolithic optimizer over a giant weighted sum.

It is an alternating constrained reconciliation procedure:

1. initialize from staged elite sets
2. conditional on activity and event-state candidates, solve the continuous-law block well
3. conditional on continuous-law candidates, refine activity and event-state blocks
4. evaluate joint admissibility and cheap joint diagnostics
5. keep only admissible elites
6. evaluate full simulation-backed diagnostics on finalists
7. select from the admissible finalist frontier with deterministic rules

This procedure treats the NIG block as a specialized subproblem instead of pretending it is just
another four-dimensional coordinate block.

## 4. Evaluation Engine

### Purpose

Define all scoring, admissibility, and simulation-backed validation logic in one place.

### Evaluation tiers

V2 must have three explicit fidelity tiers.

#### `ExactBlock`

Used when the metric can be computed without Monte Carlo.

Examples:

- exact continuous-law fit diagnostics on aggregated samples
- exact return-distribution diagnostics on observed blocks
- deterministic activity-path diagnostics when the candidate implies them exactly

#### `CoarseJoint`

Used for broad candidate screening.

Examples:

- direct coarse simulation with deterministic seeds
- horizon-matched return and activity diagnostics
- coarse rolling-window stylized facts
- lower-cost joint diagnostics that retain ranking power on aggregated targets

#### `TickJoint`

Used only for finalists.

Examples:

- full seeded simulation across the full horizon family
- full event- and activity-conditioned diagnostics
- final admissibility checks and selection diagnostics

### Coarse simulation as a first-class evaluator

Coarse simulation is not only a validation tool. It is a core evaluation engine for v2.

If coarse simulation and seeded-tick simulation are implemented correctly, then aggregated stylized
facts at matched horizons should agree closely between:

- direct coarse simulation
- seeded-tick simulation aggregated to the same horizons

That property must be exploited in the calibrator architecture.

The intended use is:

- `ExactBlock` for metrics that do not need simulation
- `CoarseJoint` for most broad joint screening on horizon-matched targets
- `TickJoint` only for finalists and for genuinely tick-level diagnostics

This is a major runtime strategy, not a small optimization detail.

### Core evaluation philosophy

The calibrator must not collapse everything into one scalar too early.

Each candidate evaluation must produce:

- validity flags
- admissibility diagnostics
- target-family metrics
- uncertainty-aware normalized distances
- optional scalar tie-break summaries

### Selection policy

Selection must proceed in this order:

1. domain validity
2. admissibility envelopes
3. Pareto frontier or lexicographic screening across target families
4. scalar tie-break only among candidates that are still meaningfully indistinguishable

### Fidelity routing policy

Every metric family must explicitly declare the cheapest valid evaluation tier.

Examples:

- block return distribution diagnostics at coarse horizons are usually `coarse-eligible`
- hourly activity profile diagnostics are usually `coarse-eligible`
- tick-event ordering diagnostics are `tick-required`
- exact continuous-law diagnostics on observed aggregated samples are `exact-eligible`

The evaluator must prefer the cheapest valid tier for each metric family and must not spend
seeded-tick budget on targets that can be screened correctly using coarse simulation.

### Admissibility envelopes

Admissibility bands must be grounded in:

- uncertainty estimates
- effective sample size
- target stability
- domain-specific mathematical constraints

They must not be hard-coded as arbitrary raw tolerances.

### Horizon treatment

There must be no hard-coded horizon blend such as `15m * 0.75 + 5m * 0.25` unless that weighting is
derived from a formal rule.

Allowed approaches:

- keep horizons separate and apply lexicographic or Pareto screening
- combine horizons using effective-sample-size-based aggregation
- combine horizons using uncertainty-normalized aggregation

The chosen rule must be explicit and justified.

## 5. Runtime and Scaling Strategy

### Purpose

Keep the calibrator practically usable on large datasets and high-core-count hardware.

### Required runtime principles

1. Precompute once, reuse everywhere.
2. Simulate once per seed, derive all horizons from that path.
3. Use large deterministic work batches to exploit parallelism.
4. Reserve expensive Monte Carlo for finalists only.
5. Cache by exact parameter identity and seed where reuse is mathematically valid.

### Required precomputations

- observed target bundles
- horizon templates
- rolling-window templates
- seasonality materialization
- trading-session templates
- known-news alignment structures
- conditioning partitions used by diagnostics

### Monte Carlo rules

Monte Carlo must be used strategically.

It is appropriate for:

- final joint reconciliation
- simulation-backed diagnostics that cannot be computed analytically
- candidate ranking only after cheap filters have removed most of the search space

It is not appropriate as the default inner-loop evaluator for every proposal.

The default simulation-backed screening tier should be coarse simulation, not seeded-tick
simulation.

### Parallelism policy

Parallelism must be explicit and stage-scoped.

Preferred work units:

- candidate families
- elite neighborhoods
- coarse simulation batches
- simulation seeds
- independent target construction tasks

Avoid:

- nested ad hoc thread pools
- serial local line-search loops that underuse available hardware
- optimizer designs that inherently expose only one to three tasks on a 24-core machine

### Caching policy

The evaluator must support at least:

- candidate summary cache
- coarse simulation summary cache
- per-seed simulation summary cache
- full finalist-evaluation cache

Cached objects must be:

- deterministic
- keyed by exact validated parameter identities and explicit evaluation context
- safe to reuse across horizons when the underlying simulated path is the same

Coarse simulation summaries must be reusable across all coarse-eligible metric families defined on
the same simulated intervals.

## 6. Diagnostic Artifact Contract

### Principle

The calibrator core is incomplete unless it emits a serious structured diagnostic artifact surface.

The interactive workbench is specified separately in
[calibration_workbench_spec.md](/mnt/c/Users/anton/source/repos/janiform/docs/design/calibration_workbench_spec.md).
This section defines what the calibrator core must produce for that workbench and for automated
inspection.

### Artifact goals

- explain why a candidate was accepted or rejected
- show where the solver spent budget
- show whether search geometry was too narrow, too broad, or badly shaped
- provide structured outputs that can be inspected by humans, scripts, LLMs, and the workbench GUI

### Required outputs per calibration run

#### Structured run bundle

Every run must emit a structured artifact bundle containing:

- configuration and seed metadata
- dataset metadata and target metadata
- staged elite sets
- joint candidate frontier snapshots
- per-candidate diagnostic payloads
- selection rationale
- simulation summaries for finalists

The bundle must be serializable to JSON without lossy ad hoc string formatting.

#### Candidate-level structured diagnostics

For every retained candidate and for optionally sampled rejected candidates, emit:

- parameter vector in canonical coordinates
- block-specific diagnostics
- admissibility status
- frontier membership
- evaluation-tier outputs
- reason for rejection if rejected

#### Search-surface diagnostics

The calibrator must emit enough structure for the workbench to materialize views of the search
geometry, including:

- one-dimensional slices through important coordinates
- two-dimensional surfaces over important coordinate pairs
- neighborhood proposal clouds
- elite frontier evolution over solver iterations

These surfaces must be exportable both as:

- visual plots
- structured numeric grids or point clouds in JSON

### Required JSON families

At minimum, v2 must emit:

- `observed_targets.json`
- `candidate_frontier.json`
- `coarse_tick_consistency.json`
- `selection_rationale.json`
- `simulation_summary.json`
- `search_surface_manifest.json`
- `runtime_profile.json`

The exact filenames may change, but the content families are required.

### LLM-friendly output rule

Every JSON artifact must be:

- self-describing
- machine-readable without reverse engineering hidden conventions
- stable in field naming
- organized so an LLM can inspect what failed and why without needing the raw code

This means:

- include metric definitions and units
- include target family grouping
- include whether a field is observed, simulated, normalized, or derived
- include ranking and admissibility annotations

## 7. Objective and Metric Taxonomy

### Core fit families

The evaluator must treat these as distinct target families:

- activity level
- activity shape
- activity clustering
- return scale
- return center mass
- return asymmetry
- return tails
- event-conditioned return behavior
- activity versus volatility coupling
- event-conditioned activity response

Each target family must also declare whether its calibration role is:

- exact-first
- coarse-screened
- tick-validated
- tick-required

These families must not be silently blended together into vague concepts such as "shape" or
"realism".

### Path-extreme diagnostics

Path-extreme functionals such as price span belong in a separate category:

- useful for diagnosing pathological behavior
- useful for windowed exceedance comparisons
- not primary optimization targets

If path-extreme diagnostics are used in admissibility, the rule must be framed in terms of
statistical path-functional comparisons, not equality to one observed realization.

## 8. Asset-Panel Acceptance Framework

V2 must be designed and judged against a broad asset panel from the start.

Minimum panel:

- EUR/USD
- BTC/USD
- one equity index future or ETF
- one single-stock instrument
- one additional non-FX instrument if available, such as a commodity or rate-linked contract

The same architecture must serve all of them.

The acceptance suite must report:

- fit quality by target family
- runtime
- parallel utilization
- coarse-versus-tick agreement quality
- diagnostic completeness
- failure modes when a fit is rejected or degrades

No asset-specific branch logic is acceptable unless the model itself explicitly distinguishes that
regime and the distinction is documented at the model level.

## 9. Code Architecture Requirements

### Required module separation

The v2 rewrite should physically separate:

- target definitions
- target estimation
- geometry definitions
- candidate generators
- solver kernels
- evaluation metrics
- admissibility policies
- diagnostics serialization
- visualization export

### Required anti-pattern bans

Do not allow:

- giant mixed-purpose modules that define targets, score them, render them, and optimize them
- `mod.rs` implementation dumping grounds
- opaque score structs with unlabeled mixed metrics
- generic optimizer wrappers that erase what the solver is actually doing

## 10. Selection of Algorithms

### Design stance

Algorithm choice is part of the architecture and must follow the actual problem geometry.

### Continuous-law block

Preferred:

- deterministic candidate generation
- elite reranking
- deterministic pattern or poll search
- coupled-direction local neighborhoods

Not preferred as the primary method:

- vanilla Powell line search over raw transformed coordinates

### Joint reconciliation

Preferred:

- alternating constrained block reconciliation
- parallel elite evaluation
- full simulation only for finalists

Not preferred:

- one monolithic local optimizer trying to solve the whole calibration surface at once

## 11. What V2 Must Explain After Every Run

After a calibration completes, the artifact bundle and the derived workbench views must be able to
answer:

- what targets were estimated from the data
- which candidates were generated and why
- which candidates were ruled out by validity
- which candidates were ruled out by admissibility
- which candidates survived onto the frontier
- which metrics separated the final few candidates
- whether runtime was dominated by candidate generation, exact scoring, or simulation
- whether coarse simulation and seeded-tick simulation agreed where they were supposed to agree
- whether search geometry appears to have missed plausible regions
- whether the final fit quality is limited by search, objective design, or data noise

If the system cannot answer those questions, the system is under-instrumented.

## 12. Delivery Rule

The rewrite is considered incomplete unless all of the following are present together:

- calibrator core architecture
- data-adaptive geometry
- block-specific solver design
- constrained evaluation and selection policy
- deterministic parallel runtime design
- structured diagnostic artifact bundle
- visualization-capable evaluation workbench as specified in
  [calibration_workbench_spec.md](/mnt/c/Users/anton/source/repos/janiform/docs/design/calibration_workbench_spec.md)

This is one design, not a core design plus optional debugging extras.

## 13. Immediate Next Design Documents To Derive From This Spec

The following detailed specs should be written directly from this document:

1. `ObservedTargetBuilder` contract and schema
2. `ContinuousLawSolver` design
3. `HawkesSolver` design
4. `JointReconciliationSolver` design
5. `EvaluationEngine` metric and admissibility schema
6. `CalibrationWorkbench` GUI, interaction, and visualization schema
7. asset-panel benchmark and acceptance schema

These derived docs should remain faithful to this spec and must not reintroduce arbitrary
multiplier-driven search or weak debugging surfaces.
