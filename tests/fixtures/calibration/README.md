# Calibration Artifact Regression Fixtures

This directory contains the bounded regression fixture set for persisted calibration artifacts.

## Files

- `calibration_artifact_round_trip_regression_fixture.json`: Versioned expected outcomes for the calibration artifact round-trip and corruption regression suite.

## Scope

The fixture covers:

- Full disk-mode round trips across dataset snapshot, stage parameter, validation metric, manifest, and experiment index artifacts
- Corruption scenarios for missing files, malformed payloads, and unsupported schema versions
- Persistence-default policy expectations for CLI and Python import surfaces

The suite is intentionally small so it stays fast and deterministic.
