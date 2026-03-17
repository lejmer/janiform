# Calibration Rearchitecture Plan

## Purpose

This document records the current state of Janiform calibration, how it differs from the
original brainstorming design, and what to change next.

The goal is to stop incremental objective tweaking and replace the current heuristic
candidate-selection approach with a calibration architecture that is closer to the model design.

This plan is intentionally practical:

- describe how calibration actually works today
- compare that to the original design intent
- identify where the current approach is structurally wrong
- define an implementation plan for a replacement

This is a working design note, not a compatibility commitment.

## Current Calibrator

Today calibration is a strict three-stage pipeline:

1. `seasonality`
2. `hawkes`
3. `return_components`

The stage orchestration lives in
[staged_runner.rs](/mnt/c/Users/anton/source/repos/janiform/crates/janiform-calibration-runtime/src/staged_runner.rs).

### 1. Seasonality stage

The seasonality stage:

- estimates deterministic calendar activity structure from timestamp events
- normalizes and freezes that structure for downstream use
- produces handoff payloads for later stages

This is implemented in
[seasonality.rs](/mnt/c/Users/anton/source/repos/janiform/crates/janiform-calibration-runtime/src/stages/seasonality.rs).

This stage is the closest part of the current system to the original design.

### 2. Hawkes stage

The Hawkes stage:

- receives precomputed candidate inputs rather than raw unconstrained parameters
- computes explicit objective terms for each candidate
- uses the optimizer adapter to select among those candidates

This is implemented in
[hawkes.rs](/mnt/c/Users/anton/source/repos/janiform/crates/janiform-calibration-runtime/src/stages/hawkes.rs).

Important characterization:

- this is not a full statistical Hawkes fit
- it is a constrained candidate-selection stage
- the main fitting object is a candidate table with hand-built diagnostics and scores

### 3. Return-components stage

The return-components stage:

- constructs summary statistics from observed open-session returns, jump candidates, jump episodes,
  close-open gaps, known-news windows, and Hawkes handoff context
- generates candidate families for:
  - continuous NIG
  - known arrivals
  - surprise arrivals
  - jump mark scales
  - activity coupling scales
- enumerates cross-products of those families into candidate inputs
- assigns explicit objective terms to each candidate
- selects a candidate with the optimizer adapter

This is implemented mainly in:

- [execution.rs](/mnt/c/Users/anton/source/repos/janiform/crates/janiform-calibration-runtime/src/stages/return_components/execution.rs)
- [continuous_fit.rs](/mnt/c/Users/anton/source/repos/janiform/crates/janiform-calibration-runtime/src/stages/return_components/continuous_fit.rs)

Important characterization:

- the continuous component is fit by synthetic distribution matching on derived calibration samples
- the news and jump layers are driven by summary targets and candidate families
- the final result is selected from discrete enumerated candidates
- the stage is only loosely joint

## What The Current Calibrator Really Is

The current calibrator is best described as:

- staged preprocessing
- summary-stat construction
- candidate-family generation
- objective-bucket scoring
- discrete model selection

It is not:

- one coherent estimation problem over a single latent model
- one likelihood-based or simulation-based joint fit
- a calibration procedure that directly matches the model’s generative semantics at every layer

That distinction matters because many of the recent pathologies are exactly what we should expect
when the model is latent and affine, but the calibrator is driven by manually engineered summary
targets and candidate grids.

## What The Brainstorm Notes Suggested

The original design notes are in
[janiform_design_brainstorm_notes.md](/mnt/c/Users/anton/source/repos/janiform/docs/design/janiform_design_brainstorm_notes.md).

The intended calibration strategy in Section 5.4 was:

1. Fit trade seasonality from timestamps only and freeze it.
2. Fit deseasonalized Hawkes parameters on trade arrivals.
3. Treat known-news timestamps as anchored exogenous events.
4. Infer surprise-news arrivals from residual jumps and fit a surprise-news arrival process with
   calendar covariates.
5. Fit the shared jump-mark law from event-conditioned jump magnitudes.
6. Fit news-to-Hawkes coupling from post-shock activity response.
7. Run a constrained joint refinement over Hawkes plus news parameters.

The brainstorm therefore assumed:

- staged initialization
- then statistically meaningful sub-fits per layer
- then joint constrained refinement

## Current System Versus Brainstorm

### Where the current implementation matches

1. Seasonality is estimated first and then frozen.
2. Hawkes is downstream of seasonality.
3. Known-news timestamps are treated as explicit inputs.
4. Open-window jumps and close-open gaps are distinguished.

### Where it only partially matches

1. Hawkes fitting
   - intended: parameter fit on deseasonalized arrivals
   - current: candidate generation plus score-based selection

2. Mark-law fitting
   - intended: fit from event-conditioned jump magnitudes
   - current: closer than before, but still embedded in candidate-family scoring rather than a
     dedicated fit

3. News-to-Hawkes coupling
   - intended: fit from post-shock activity response
   - current: partially uses this evidence, but through summary targets and family scoring

### Where it clearly diverges

1. Surprise-news arrival model
   - intended: fit `nu_surprise(t)` as a point process with calendar covariates
   - current: infer surprise episodes and turn them into summary rates and candidate families
   - missing: actual covariate-driven arrival-intensity estimation

2. Joint refinement
   - intended: constrained refinement over Hawkes plus news parameters
   - current: no real joint refinement
   - the layers are coupled mostly through handoff summaries, not by one coherent objective over
     all relevant parameters

3. Continuous-return calibration
   - intended model: NIG in business time with side-specific integrated intensity exposure
   - current fit: derived calibration sample plus synthetic-profile scoring
   - this is a useful approximation, but it is still disconnected from a proper layer-wise or
     joint estimation procedure

## Main Diagnosis

The central problem is no longer just "the objective needs tuning."

The central problem is that the calibration architecture is more heuristic than the model design.

The current approach has several structural weaknesses:

1. Too much information is compressed into summary targets before fitting.
2. Too many layers are fit through candidate-family enumeration rather than explicit statistical
   models.
3. Surprise-news intensity is not modeled as an actual arrival process with covariates.
4. There is no genuine joint refinement step that reconciles trade arrivals, news arrivals, jump
   marks, coupling, and continuous returns.
5. The return-components stage has become a catch-all layer where modeling compromises accumulate.

## Rearchitecture Direction

The replacement should move closer to the brainstorm design.

The new architecture should be:

1. `seasonality` fit
2. `hawkes` fit
3. `news arrivals` fit
4. `news marks` fit
5. `news-to-hawkes coupling` fit
6. `continuous return law` fit
7. constrained joint refinement

This still allows staged execution, but each stage should fit a model object rather than only
generate scored candidate tables.

## Proposed Replacement Architecture

## Layer 1: deterministic seasonality

Keep the current high-level approach.

Requirements:

- fit from timestamps only
- freeze after estimation
- preserve mean-one normalization over the training window

This is already the strongest part of the current pipeline and should remain mostly intact.

## Layer 2: Hawkes fit on deseasonalized arrivals

Replace the current Hawkes candidate-selection mindset with an actual fit over the Hawkes
parameterization.

Target:

- fit `bar_lambda`, `beta`, `alpha_ij`
- use deseasonalized arrivals only
- keep the current stability and positivity constraints
- preserve structured parameterization choices where they materially improve identifiability

The current candidate machinery can still be used as initialization or coarse screening, but it
should not be the primary estimation method.

## Layer 3: News-arrival calibration

Split news-arrival estimation into two explicit parts.

### Known news

- event times are fixed by the known-news calendar
- fit only response and mark behavior around those anchored events

### Surprise news

- infer surprise event candidates from residual open-window jumps and close-open gaps after
  removing known-news windows
- cluster those into event episodes
- fit a surprise-arrival intensity model with calendar covariates:
  - day of week
  - day of year
  - holiday effects
  - open versus closed session

This is one of the largest missing pieces in the current implementation.

## Layer 4: Shared news mark-law fit

Fit the shared mark law directly from event-conditioned shock magnitudes:

- open-window first-tick jumps
- close-open gaps
- known-news windows
- inferred surprise-news windows

Fit:

- `k_U`
- `theta_U`
- `sigma_G`
- `p`
- `eta_plus`
- `eta_minus`

One shared law remains the default.

Only split known versus surprise mark laws if diagnostics justify it.

## Layer 5: News-to-Hawkes coupling fit

Fit `rho_ij` or the chosen structured coupling parameterization from post-event activity response.

This layer should:

- measure excess activity after known and inferred surprise events
- compare to Hawkes-without-news counterfactual activity
- fit coupling from that response

This should be a real coupling fit, not only a family ranking on a scalar target.

## Layer 6: Continuous return law fit

Keep the latent target:

- fit the trading-time return law, not arbitrary clock-time bars

But change the fitting mindset:

- the continuous layer should be estimated as one model layer, not mainly as one family inside a
  giant return-components candidate cross-product

The recent work on equal-business-time blocks and exact rescoring is still useful.
That should become part of the estimation machinery for the continuous layer itself.

Important requirement:

- the fitting object must stay aligned with the runtime object

This means calibration should match the same side-mixed business-time return mechanism that the
runtime generates.

## Layer 7: Joint constrained refinement

After staged sub-fits, run a constrained joint refinement across:

- Hawkes parameters
- surprise-news arrival parameters
- mark-law parameters
- news-to-Hawkes coupling
- continuous NIG parameters

This is the key element missing today.

The purpose is not to start from scratch jointly.
The purpose is to reconcile the staged estimates inside one coherent constrained objective.

## What We Should Stop Doing

The replacement architecture should avoid the following as primary mechanisms:

1. Using large candidate cross-products as the main fit engine.
2. Using only summary-rate targets where an actual point-process fit is intended.
3. Letting one catch-all return-components stage absorb every unresolved modeling compromise.
4. Treating coarse candidate selection as a substitute for joint refinement.

Some of these tools may remain useful for:

- initialization
- guardrail diagnostics
- bounded coarse search

But they should not define the calibration architecture.

## Recommended Implementation Phases

## Phase 1: Freeze the current exact scorer as a diagnostic tool

Keep the recent exact candidate evaluator and playground diagnostics.

Do not throw this away.

It is useful for:

- regression checks
- comparing staged estimates to manual edits
- validating that any replacement objective aligns better with observed behavior

## Phase 2: Split return-components into explicit subproblems

Refactor the current return-components stage into internal submodules:

- `news_arrivals`
- `news_marks`
- `news_activity_coupling`
- `continuous_returns`

This can still run under the same top-level stage boundary initially, but the fitting logic should
be separated into actual layer-specific estimation flows.

## Phase 3: Implement a real surprise-news arrival fit

This is the most important missing model layer.

Deliverables:

- inferred surprise-news episode dataset
- covariate design matrix or equivalent structured representation
- estimated surprise-arrival intensity model
- diagnostics for explained versus unexplained jump episodes

## Phase 4: Implement direct mark-law and coupling fits

Replace family-ranking logic with dedicated estimation routines for:

- shared jump-mark law
- news-to-Hawkes coupling

The current family machinery can still provide coarse initialization if useful.

## Phase 5: Replace continuous candidate enumeration with explicit estimation plus refinement

Use the current exact continuous scorer as a validation target, but move the continuous layer
toward:

- explicit transformed-parameter optimization
- real local refinement
- better coupling with the joint stage

## Phase 6: Add joint constrained refinement

This is the end-state milestone.

The system should:

- start from staged sub-fits
- refine jointly under one constrained objective
- preserve deterministic seeds and reproducible diagnostics

## Acceptance Criteria

We should treat the rearchitecture as successful when:

1. Surprise-news intensity is fit as a real arrival model, not only inferred episode frequency.
2. Mark-law parameters are fit from data rather than mostly chosen through family grids.
3. Coupling is fit from explicit post-shock activity response modeling.
4. Continuous-return calibration is a proper layer fit rather than a side-effect of candidate
   cross-product search.
5. Joint refinement exists and materially changes parameters relative to staged initialization.
6. Manual playground improvements are no longer obviously better than the auto-fit under the exact
   scorer and diagnostics.

## Immediate Next Step

Do not continue objective micro-tuning first.

Start by restructuring the current return-components stage into explicit internal subproblems and
implement the surprise-news arrival model as a real fitted layer.

That is the highest-leverage change because it addresses the largest architectural gap between the
current calibrator and the original design.
