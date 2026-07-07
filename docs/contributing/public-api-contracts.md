# Maintaining public API contracts

Public APIs are contracts with Janiform users. Changes to those contracts should be intentional, reviewable, documented, and verified in the same pull request as the code change.

The public API guardrails make accidental API drift visible. They snapshot the supported Python API surface, compare it with the checked-in fixture, and require an approval manifest entry for every intentional snapshot update. The guardrails do not decide whether an API change is compatible. They make the change explicit so reviewers can apply release planning and API design review.

## Public API policy

- Treat public APIs as deliberate contracts.
- Avoid changing public APIs unintentionally.
- Keep public API changes in the same pull request as the matching tests, fixtures, generated references, examples, and docs.
- Do not add compatibility wrappers, aliases, fallback parsers, or deprecated alternate payloads as a shortcut around API design review.
- Prefer one clear public contract over parallel old and new shapes.
- Update in-repo call sites and examples when a public contract changes.
- Document behavior as the current project shape. Do not word it a migration note, unless in a context that is explicitly documenting historical API migrations.

If a compatibility layer is needed, document its purpose in the code, docs, and pull request, and mark the old path as deprecated.

Breaking changes require explicit release planning and reviewer agreement. Additive changes still require design review, documentation, and guardrail approval because they expand the supported surface.

## What counts as public API

For Python, the tracked public API surface is defined by `PUBLIC_CONTRACT_MODULE_NAMES` in `tests/support/public_api_contract_guardrails.py`. Each tracked module must expose its public names through `__all__`.

The snapshot records:

- package root exports
- tracked public modules
- module `__all__` exports
- function signatures
- contract model and dataclass constructor signatures
- public contract model fields
- public methods on exported classes and contract models
- enum members and values
- exported exception classes
- public schema and version constants discovered in tracked modules
- constant values when they are part of an exported public module

Rust public APIs are governed by crate exports, visibility, `///` documentation, and the Rust conventions. The current public API snapshot guardrail is focused on the Python user-facing surface, including Python contracts and PyO3-backed entrypoints exposed through Python modules, since Rust is internal only.

The command-line interface is also a public interface. CLI changes should update command tests and generated CLI references, but they are not represented in `public_api_contract_snapshot.json`.

## Guardrail files

The public API guardrail is made up of these files:

| File                                                                      | Purpose                                                                  |
|---------------------------------------------------------------------------|--------------------------------------------------------------------------|
| `tests/support/public_api_contract_guardrails.py`                         | Defines the tracked modules and builds deterministic API snapshots.      |
| `tests/test_api_contract_guardrails.py`                                   | Verifies the current API matches the checked-in snapshot and manifest.   |
| `tests/fixtures/python_api_contracts/public_api_contract_snapshot.json`   | The approved snapshot of the tracked Python public API surface.          |
| `tests/fixtures/python_api_contracts/public_api_contract_guardrails.json` | The approval manifest for the checked-in snapshot hash.                  |
| `scripts/update_public_api_contract_snapshot.py`                          | Regenerates the public API snapshot fixture and prints its SHA-256 hash. |

The tests enforce two conditions:

- The current public API snapshot must match `public_api_contract_snapshot.json`.
- The guardrail manifest must approve the checked-in snapshot hash and include a rationale in its latest `change_log` entry.

## What the guardrails do not do

The guardrails do not prove that a change is compatible, complete, or well-designed. They are a review aid, not a substitute for API design review.

The guardrails do not update generated references automatically. Run the reference generator when the public API documentation needs to change.

The guardrails do not make compatibility wrappers or deprecation paths appropriate by default. If a compatibility layer is needed, document the compatibility purpose in the code, docs, and pull request.

## When to update the snapshot

Update the snapshot when the public Python API surface changes intentionally. Common examples include:

- adding, removing, or renaming a public export
- changing a public function or method signature
- changing contract model fields or defaults
- changing enum members or enum values
- changing public schema or version constants
- adding a new public module to the tracked API surface
- changing the guardrail snapshotter so it records a different public contract shape

Do not update the snapshot just to silence a failing test. First decide whether the API drift is intentional. If it is not intentional, fix the code so the current snapshot returns to the approved contract.

## Updating the snapshot fixture

Run these steps from the repository root with the project environment active.

1. Make the intended API change.
2. Update `__all__` exports and `PUBLIC_CONTRACT_MODULE_NAMES` when the public surface changes.
3. Update in-repo call sites, tests, examples, generated references, and docs that describe the changed contract.
4. Regenerate the snapshot fixture.

   On Linux or WSL:

   ```bash
   python3 scripts/update_public_api_contract_snapshot.py
   ```

   On Windows:

   ```powershell
   python scripts/update_public_api_contract_snapshot.py
   ```

5. Copy the printed SHA-256 hash into `approved_snapshot_sha256` in `tests/fixtures/python_api_contracts/public_api_contract_guardrails.json`.
6. Append a `change_log` entry to `public_api_contract_guardrails.json`.

The latest `change_log` entry must approve the new snapshot hash:

```json
{
  "approved_on": "YYYY-MM-DD",
  "docs_updated": [
    "docs/reference/python-api.md"
  ],
  "rationale": "Explain why this public API shape is the right contract.",
  "snapshot_sha256": "<printed snapshot hash>",
  "summary": "Short description of the approved API change.",
  "updated_surfaces": [
    "janiform.calibration"
  ]
}
```

Use `approved_on` as an ISO 8601 date. Keep `summary` concise. Use `rationale` for the design reason, not a restatement of the diff. List the modules or public surfaces affected in `updated_surfaces`. Include `docs_updated` when user-facing docs or generated references changed.

## Updating generated references

The Python API reference is generated from the same public API snapshot machinery. When the public Python API changes, update the generated reference page with:

```bash
python3 scripts/generate_docs_references.py
```

On Windows, use:

```powershell
python scripts/generate_docs_references.py
```

Review the generated diff. The reference page should describe the current public contract, and the guardrail manifest should record that the reference docs were updated when applicable.

## Verifying the change

For a focused public API contract change, run:

```bash
python3 -m pytest tests/test_api_contract_guardrails.py tests/test_public_api_surface_map.py tests/test_experimental_api_surface.py
python3 scripts/generate_docs_references.py --check
```

On Windows, use `python` instead of `python3`.

Broaden verification when the contract change touches workflow behavior, Rust bindings, serialization, CLI behavior, or generated artifacts outside the Python API reference.

## Reading guardrail failures

Snapshot diffs mean the current public API no longer matches the approved fixture. Review the diff and decide whether the drift is intentional.

Hash mismatches mean `public_api_contract_snapshot.json` and `public_api_contract_guardrails.json` are out of sync. Regenerate the snapshot when the API change is intentional, then update both the top-level `approved_snapshot_sha256` and the latest `change_log` entry.

`__all__` errors mean a tracked public module is not exposing its public names in the expected form. Public Python modules should use a list of strings.

Import errors usually mean `PUBLIC_CONTRACT_MODULE_NAMES` points at a module that is missing, renamed, or unavailable in the active environment.

## Updating the guardrail itself

Changing the guardrail is different from approving a normal API snapshot update. A guardrail change modifies what Janiform considers part of the captured public contract.

When updating the guardrail:

- update `tests/support/public_api_contract_guardrails.py`
- add or adjust tests in `tests/test_api_contract_guardrails.py`
- regenerate `public_api_contract_snapshot.json` only after reviewing the new snapshot shape
- add a manifest entry explaining the guardrail behavior change
- update this page when contributors need a different workflow

Only change guardrail schema or document version constants when the meaning of the snapshot format or tracked surface changes. Do not change those constants for ordinary public API additions, removals, renames, or signature updates.

## Pull request checklist

Before opening a pull request that changes a public API, verify that:

- the public API change is intentional
- compatibility implications are described in the pull request
- in-repo call sites are migrated
- user-facing docs and generated references are current
- the public API snapshot fixture is regenerated when needed
- the guardrail manifest approves the new snapshot hash with a clear rationale
- focused guardrail tests pass
