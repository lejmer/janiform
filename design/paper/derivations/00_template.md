# Derivation title

Status: Draft
Plan item: `Exact derivation name from design/paper/plan.md`
Depends on: `None` or linked derivation files
Feeds: `Paper section, appendix, proposition, plot, or claim`

## Purpose

State the specific paper need this derivation satisfies. Keep this focused on the mathematical role, not the implementation.

## Setup

Define the time interval, conditioning information, state variables, parameters, and random objects used in the derivation.

Use this section to make all conventions explicit:

- Conditioning sigma-field or initial state.
- Measure used in the derivation.
- Clock, seasonality, or schedule convention.
- Normalization convention.
- Domain restrictions needed before any transform is evaluated.

## Notation

| Symbol | Meaning | Units or domain |
|--------|---------|-----------------|
| `...`  | ...     | ...             |

## Assumptions used

| Assumption | Where it is used | Why it is needed |
|------------|------------------|------------------|
| ...        | ...              | ...              |

## Result

State the final result in the form that should appear in the paper.

If the result is a proposition, use:

**Proposition.** ...

If the result is an identity or construction, use:

**Identity.** ...

## Derivation

Work through the derivation step by step.

Keep each algebraic transition justified. If a step depends on independence, conditioning, affine closure, a moment-domain constraint, or a normalization, name that reason explicitly.

## Domain and validity

List the exact conditions under which the result is valid.

Include transform domains, positivity constraints, stability requirements, integrability requirements, and any terminal or boundary conditions.

## Consequences

Record the direct implications that should be reused elsewhere in the paper.

- Consequence 1.
- Consequence 2.

## Checks

Describe how the result should be checked analytically or numerically.

| Check | Expected result | Evidence location |
|-------|-----------------|-------------------|
| ...   | ...             | ...               |

## Open questions (if any)

- ...
