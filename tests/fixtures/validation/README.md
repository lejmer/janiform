# Shared Validation Fixture Manifest

This directory contains the canonical fixture manifest used by both Rust and Python validation tests.

## File

- `validation_fixture_manifest.json`: Manifest for shared fixture discovery, versioning metadata, and deterministic content hashing.

## Manifest Contract

Top-level fields:

- `schema_version`: Manifest schema identifier.
- `manifest_id`: Stable manifest identifier.
- `manifest_version`: Semantic manifest version for v0.x updates.
- `versioning_policy`: Machine-readable update policy metadata.
- `determinism_policy`: Machine-readable load-order and hash policy metadata.
- `fixtures`: Fixture entries consumed by both test stacks.

Each fixture entry defines:

- `fixture_id`: Stable fixture set identifier.
- `domain`: Fixture domain, currently `calendar` and forward-extensible to `seasonality`.
- `fixture_schema_version`: Target fixture document schema version.
- `relative_path`: Fixture document path relative to `tests/fixtures`.
- `exchange_codes`: Exchanges covered by the fixture.
- `content_hash`: FNV-1a 64-bit hash of the fixture payload bytes, encoded as lowercase hex.

## Determinism Rules

- Entries are loaded in lexicographic order by `fixture_id`.
- Fixture payloads must match `content_hash` before parsing.
- Hash algorithm policy is declared in `determinism_policy.content_hash_algorithm`.
