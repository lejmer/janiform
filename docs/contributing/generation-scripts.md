# Generation scripts

Janiform keeps several generated files checked into the repository. Do not edit generated files directly. Edit the source data or generator, run the matching script, and commit both the source change and the generated output.

Run scripts from the repository root with the project environment active. For setup, see [Set up a development environment](setup.md).

## Regular generators

- `scripts/generate_bundled_presets.py`
  - **Generates:** Built-in preset JSON under `python/janiform/calibration/_bundled_presets/generated/`.
  - **Use it when:** Preset source files under `presets/sources/` change, or preset artifact contracts change.
- `scripts/generate_crate_readmes.py`
  - **Generates:** `README.md` files under `crates/*/`.
  - **Use it when:** A crate is added, removed, renamed, republished, or its `Cargo.toml` metadata changes.
- `scripts/generate_docs_references.py`
  - **Generates:** Generated reference pages under `docs/reference/`.
  - **Use it when:** The CLI command surface or public Python API reference surface changes.
- `scripts/generate_internal_crate_graph_policy.py`
  - **Generates:** `tests/fixtures/rust_architecture/internal_crate_graph_policy.json`.
  - **Use it when:** Workspace crates, internal crate dependencies, crate classifications, or umbrella facade modules change.
- `scripts/generate_third_party_notices.py`
  - **Generates:** `THIRD_PARTY_NOTICES.txt`.
  - **Use it when:** Rust dependencies behind the packaged native extension change, or dependency license metadata changes.
- `scripts/update_public_api_contract_snapshot.py`
  - **Generates:** `tests/fixtures/python_api_contracts/public_api_contract_snapshot.json`.
  - **Use it when:** The public Python API surface changes intentionally. See [Maintaining public API contracts](public-api-contracts.md).
- `scripts/update_workflow_response_schema_snapshot.py`
  - **Generates:** `tests/fixtures/python_api_contracts/workflow_response_schema_snapshot.json`.
  - **Use it when:** Workflow response payload shapes change intentionally.

> **NOTE:** For Rust crate graph work, treat any added, removed, or reordered workspace crate dependency in a `crates/*/Cargo.toml` file as an internal crate dependency change. Run `python scripts/generate_internal_crate_graph_policy.py` in the same change so the checked-in policy fixture records the approved graph.

## Example and benchmark artifacts

These scripts can do more work than the regular metadata generators. Run them deliberately and avoid running multiple expensive commands at the same time.

- `scripts/generate_synthetic_option_snapshot.py`
  - **Generates:** Synthetic option snapshot artifacts under `.artifacts/examples/option_surfaces/` by default.
  - **Use it when:** The synthetic EUR/USD option-surface example needs a fresh snapshot, or its generation policy changes.
- `scripts/generate_real_data_seeded_runtime_smoke_report.py`
  - **Generates:** `tests/fixtures/benchmarks/real_data_seeded_runtime_smoke_report.json`.
  - **Use it when:** The smoke benchmark fixture changes, or the checked-in smoke report needs to reflect an intentional runtime/report contract change.

## Check mode

Some generators support `--check`. In check mode, the script compares the checked-in output with the expected generated output and exits with a non-zero status if it is stale.

```bash
python scripts/generate_crate_readmes.py --check
python scripts/generate_docs_references.py --check
python scripts/generate_internal_crate_graph_policy.py --check
python scripts/generate_third_party_notices.py --check
```

CI runs the important check-mode generators. If one of these checks fails locally or in CI, run the same script without `--check`, inspect the diff, and commit the generated output when the source change is intentional.

## Common workflow

1. Change the source data, crate metadata, public API, or generator.
2. Run the matching generation script.
3. Review the generated diff as part of the code review, not as mechanical noise.
4. Run the relevant `--check` command when the script supports it.

Generated files usually contain a header such as `DO NOT EDIT`. Treat that as a pointer to the generator, not as a reason to skip review.
