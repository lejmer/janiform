# Python API Contract Compatibility And Version Guardrails

Document version: `python.api_contract_guardrails.pre_v0_1.control_plane_reset.v1`

Status: active for the current pre-v0.1 milestone on March 26, 2026.

## Purpose

This guide defines how Janiform tracks intentional public Python contract changes during the
pre-v0.1 phase. Compatibility is not preserved by default in this repo, but breaking changes still
need to be explicit, reviewed, and documented.

## What counts as a breaking public-contract change

For the guardrail snapshot, a breaking change is any change to the approved public Python surface in:

- the package-root export set for `janiform`
- the `__all__` export set of public package roots and public submodules listed in
  `docs/development/guides/python_public_api_surface_map_and_namespace_policy.md`
  including `janiform.cli` and the supported command modules
  - underscore-prefixed helper names are not public contract exports and must not appear in `__all__`
- callable signatures for exported public functions
- constructor-visible field signatures for exported contract models
  - underscore-prefixed runtime-only dataclass fields are excluded from the guardrail snapshot
- enum member names or enum member values for exported enums
- exported constant values that act as policy identifiers, execution defaults, or contract identifiers
- schema identifier strings defined in public contract modules, including non-exported module constants used by
  serialization or validation paths

These checks are intentionally narrow.
Internal helper refactors, private-module changes, and non-public implementation details do not
count as breaking changes unless they modify one of the tracked public contract records above.

One symbol may intentionally appear in more than one tracked public module when the design uses a
curated convenience import plus a natural domain-module import. That is not treated as policy drift.
The guardrail tracks each approved public module separately.

The `janiform.experimental` namespace is excluded from these guardrails by default.
Preview endpoints are covered by the opt-in lifecycle policy in
`docs/development/guides/python_experimental_api_policy.md` until they are promoted into
the primary public surface.

## Guardrail artifacts

The verification baseline is stored in:

- `tests/fixtures/python_api_contracts/public_api_contract_snapshot.json`
- `tests/fixtures/python_api_contracts/public_api_contract_guardrails.json`

The snapshot contains the approved public package-root exports, public-module exports, signatures,
enum values, constant identifiers, and schema identifiers.

Signature capture uses stringified annotations rather than evaluated runtime objects so the approved snapshot stays
stable across supported Python environments, including Linux and Windows developer workflows.

When tracked public signatures need to describe native-owned runtime handles or native result objects,
they must do so through Janiform-owned protocol boundary types rather than concrete `janiform._native`
implementation classes.

The guardrail manifest records:

- the approved snapshot hash
- the approval policy identifier
- the public API surface document version
- a change log entry with date, summary, rationale, and docs updated

## Review checklist for this milestone

Every public-surface review must reject a change that:

- introduces a new Python-owned tuple-of-record bulk evidence boundary
- adds a new `Sequence[Mapping[...]]` market-data or seasonality ingestion contract on the
  recommended path
- makes `PreparedMarketDataCalibration`, `JaniformFitReport`, or staged results embed bulk
  evidence instead of artifact references, handles, or summaries
- preserves both the old tuple-heavy contract and the new artifact-backed contract in the same
  supported surface
- adds a new public wrapper whose entire body is payload forwarding or one-line delegation
- expands a curated package root until it stops reading like a small supported overview

## Verification command

Run:

```bash
PYTHONPATH=python .venv-wsl/bin/python3.14 -m pytest tests/test_api_contract_guardrails.py tests/test_public_api_surface_map.py
```

`tests/test_api_contract_guardrails.py` fails when code drifts from the approved snapshot or when
the approval manifest does not approve the checked-in snapshot hash.

## Procedure For Approved Breaking Changes

When a public contract break is intentional and approved:

1. Update the code, in-repo call sites, tests, and user-facing docs in the same change.
2. If the change adds, removes, or renames a public submodule, update
   `docs/development/guides/python_public_api_surface_map_and_namespace_policy.md` first so the
   snapshot continues to reflect an intentional policy decision.
   If the change alters the Python control-plane or Rust data-plane split, update
   `docs/development/guides/python_control_plane_and_rust_data_plane_boundary.md` in the same
   change.
3. Regenerate the approved snapshot:

```bash
PYTHONPATH=python .venv-wsl/bin/python3.14 scripts/update_public_api_contract_snapshot.py
```

4. Update `tests/fixtures/python_api_contracts/public_api_contract_guardrails.json`:
   - set `approved_snapshot_sha256` to the new script output
   - append a new `change_log` entry
   - record the approval date, summary, rationale, and updated docs
5. Run the guardrail tests and the relevant workflow contract tests.

## Schema version policy

Pre-v0.1 contract changes do not get compatibility shims. When a breaking change improves
correctness, package design, or clarity, replace the old contract directly and update the snapshot.

Do not bump schema identifiers during these refactors unless a specific task explicitly requires it.
Schema identifiers remain at `v1` under the repo-wide pre-v0.1 contract policy.
