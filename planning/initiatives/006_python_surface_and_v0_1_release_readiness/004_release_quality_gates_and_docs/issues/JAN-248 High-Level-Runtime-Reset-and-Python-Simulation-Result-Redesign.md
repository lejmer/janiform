# High-Level Runtime Reset and Python Simulation Result Redesign

## Summary
Reset the normal high-level workflows so `fit`, `simulate`, and `price` stop using contract-shaped JSON transport as their execution path. Rust should own the heavy runtime work for high-level workflows. Python should remain a thin boundary for validation, defaults, user-facing reporting, and artifact orchestration.

For library ergonomics, keep `fit()` returning `JaniformModel` and `price_option()` returning a small Janiform pricing result, but redesign `simulate()` to return a Janiform simulation result object rather than the current custom payload wrappers. That result object must expose `to_numpy()`, `to_pandas()`, and `to_parquet()` as first-class methods so the common Python workflow feels native without forcing pandas into the core execution boundary.

This implementation is one coherent reset, not a compatibility layer. Keep schema/version identifiers at `v1`, keep current top-level CLI/library shapes unless a direct simplification is required, and update in-repo callers/tests/docs in the same change set.

## Implementation Changes
- Replace the high-level fit runtime path with a Rust-owned market-data workflow.
    - Rust owns source loading, normalization, session resolution, preprocessing, and staged calibration input construction for normal `fit`.
    - Remove the current repeated source-load/normalization passes from the high-level fit path.
    - Python `JaniformEstimator.fit(...)` becomes validation/default injection, progress/output, warning emission, and model/report artifact handling only.

- Replace high-level JSON execution transport with typed pybind boundaries across the workflow family.
    - `fit`: no `preprocessing_request_json` or `execution_input_json` in the high-level path.
    - `price`: replace `execution_input -> JSON -> Rust -> JSON -> Python` with typed pybind calls.
    - `simulate` coarse path: replace JSON request/response transport with typed pybind calls.
    - `simulate` seeded path: remove remaining JSON/config transport where still used for high-level workflow execution.
    - Explicit contract commands may still parse/emit JSON at the user boundary, but after parsing they should call typed/native internals wherever feasible.

- Redesign the library simulation result surface around one Janiform-owned result object.
    - `JaniformModel.simulate(..., output_path=None)` returns a simulation result object, not a raw custom execution payload.
    - That object must expose:
        - `to_pandas()`
        - `to_numpy()`
        - `to_parquet(path)`
    - Keep metadata, provenance, diagnostics, and any workflow-specific information on the Janiform result object instead of discarding them into a bare DataFrame.
    - `output_path` should still prefer direct streaming to the final file rather than materializing everything in memory first.
    - CLI `simulate` remains file-oriented by default.

- Keep explicit professional commands first-class, but split their purpose clearly.
    - High-level workflows: typed/native runtime.
    - `fit plan`, `fit staged`, and related professional/debug/document surfaces: JSON/document-oriented UX.
    - Where those explicit commands execute real work, route them into typed/native internals after validation instead of reusing JSON-string transport as the runtime boundary.

- Preserve the recent UX fixes while updating stage naming to match reality.
    - Keep stage-row progress rendering and final total summary.
    - Rename steps so they describe actual work after the runtime reset.
    - Keep compact pretty-printed JSON artifacts and simulation streaming semantics.

## Public API and Interface Changes
- `JaniformEstimator.fit(...)`
    - Return type stays `JaniformModel` or `(model, report)` when `report=True`.
    - No new required parameters.

- `JaniformModel.simulate(...)`
    - Return type changes to a Janiform simulation result object for in-memory use.
    - The result object becomes the canonical Python library surface for simulated tabular data plus metadata.
    - Required convenience methods:
        - `to_pandas()`
        - `to_numpy()`
        - `to_parquet(path)`

- `JaniformModel.price_option(...)`
    - Keep returning a Janiform pricing result object.
    - Internal execution boundary changes to typed/native, but user-facing call shape stays the same.

- CLI
    - No new top-level commands.
    - No change to the durable-output defaults already established.

## Test Plan
- Add Rust and pybind tests for the new typed high-level boundaries for fit, price, coarse simulate, and seeded simulate config transport.
- Add regression coverage that one high-level fit run does not perform multiple full source-load/normalization passes.
- Update library simulation tests to assert the new result object surface and its `to_pandas()`, `to_numpy()`, and `to_parquet(path)` methods.
- Keep high-level CLI/library workflow tests green for `fit`, `simulate`, and `price`.
- Keep explicit command tests green for `fit plan` and `fit staged`.
- Add or update progress-output tests so step wording matches the new runtime stages.
- Run full Python suite and relevant Rust/pybind suites, with Windows coverage for the affected high-level workflow paths.

## Assumptions and Defaults
- High-level runtime target: Rust-owned execution path, not a Python NumPy/Arrow-first redesign.
- Explicit commands remain supported and professional, but their internal execution should converge on typed/native calls where possible.
- `simulate()` library default is a Janiform result object with `to_numpy`, `to_pandas`, and `to_parquet`, not a bare pandas DataFrame.
- JSON remains for persisted artifacts and explicit document-boundary workflows, not for normal internal execution transport.
- Pre-release policy applies fully: remove obsolete paths directly, do not add compatibility shims, and do not bump schema versions.
