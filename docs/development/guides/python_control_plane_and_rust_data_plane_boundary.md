# Python Control Plane And Rust Data Plane Boundary

Document version: `python.control_plane_rust.data_plane.v1`

Status: active for the current pre-v0.1 milestone on March 26, 2026.

## Purpose

This guide defines the implementation boundary for production Janiform workflows.

- Python owns the control plane.
- Rust owns the data plane.
- Bulk empirical datasets stay native-owned, artifact-backed, or columnar-backed.
- Public Python dataclasses are for small config and summary objects only.

## Required boundary rules

- Keep request, preset, policy, and small summary contracts in Python.
- Move empirical evidence derivation, translation, and preparation into Rust whenever the payload
  grows with source data size or horizon length.
- Represent prepared calibration inputs as native-backed handles or file references, not Python
  tuples of record dataclasses.
- Use `from __future__ import annotations` in Python modules that declare public or shared contract
  types, and annotate native-backed handles with their concrete exported types instead of `object`
  placeholders.
- Keep reports summary-only. Reports may reference artifacts and diagnostics, but they must not
  duplicate bulk evidence payloads.
- Keep high-level CLI and Python surfaces explicit. They should accept file paths, policies,
  presets, and small config objects, not giant staged evidence JSON documents.

## Default calibration path

The supported production path is:

1. `MarketDataCalibrationRequest`
2. native preparation and translation
3. `PreparedMarketDataCalibration`
4. staged execution against the prepared handle or persisted prepared artifact
5. `JaniformModel` and `JaniformFitReport`

The default high-level path must not materialize a canonical Python-owned staged evidence graph.

## Advanced staged path

The expert staged path remains public, but it is now artifact-backed:

- `StagedCalibrationExecutionInput` contains:
  - `request`
  - `prepared_payload_file`
  - optional `runtime_resources`
- The prepared payload file references a typed staged execution artifact.
- The expert path is for orchestration control, not for Python-owned bulk evidence assembly.

## Review checklist

Reject a change when it:

- adds a new Python tuple-of-record bulk evidence contract
- adds a new `Sequence[Mapping[str, Any]]` bulk-data default path
- introduces stringed type hints or `object` placeholders where a concrete public contract or
  native-handle type is already known
- makes a report or runtime artifact eagerly return a full dataset
- preserves an old tuple-heavy contract alongside a new artifact-backed one
- introduces a compatibility shim instead of replacing the weak boundary directly
