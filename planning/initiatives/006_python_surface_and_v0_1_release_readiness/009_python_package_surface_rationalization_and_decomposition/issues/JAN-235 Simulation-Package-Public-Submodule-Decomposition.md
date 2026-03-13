## Simulation Package Public Submodule Decomposition

### Description
Refactor `janiform.simulation` into named public submodules that reflect the real workflow structure, so coarse simulation, seeded simulation, and shared contracts are discoverable without routing everything through one package `__init__.py`.

### Implementation details
- Replace `_coarse`, `_seeded`, and `_shared` with a structured package layout built around meaningful names such as `coarse`, `seeded`, and `shared` or a similarly clear decomposition.
- Decide which simulation types and functions belong in the package root as convenience exports and which should primarily live in explicit submodules.
- Keep genuinely internal helpers private under underscore names only when they are not meaningful domain modules.
- Split any large simulation modules further if the first decomposition still leaves monolithic files.
- Update runtime and contract tests, docs, examples, and internal imports to use the new structure directly.

### Acceptance Criteria
- `janiform.simulation` no longer depends on underscore-prefixed modules for its main domain structure.
- Public simulation concepts are available from named public submodules that match the workflow organization.
- The package root continues to offer a curated convenience surface for common imports without being the only reasonable way to navigate the package.
- The previous underscore module names are removed directly instead of being preserved in parallel.
- Public docs and examples show the intended import style for both convenience and explicit-domain usage.

### Unit tests
- Refresh public-surface tests and snapshots for the simulation package.
- Re-run simulation contract and parity tests after the import-path refactor.
- Add import tests that validate the intended public submodule paths.

### Notes/Constraints
- Keep simulation package structure aligned with runtime concepts, not with current file accidents.
- If a symbol still has to be lazy-imported to avoid circularity or import cost, document that clearly and keep the mechanism minimal.
