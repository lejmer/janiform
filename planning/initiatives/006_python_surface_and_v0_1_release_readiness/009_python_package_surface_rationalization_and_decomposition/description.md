## Project 009: Python Package Surface Rationalization And Decomposition

Replace the current underscore-heavy Python package layout with a more maintainable and discoverable structure for `janiform.calibration`, `janiform.cli`, and `janiform.simulation`, so v0.1 does not freeze an arbitrary facade-only namespace policy that already makes large files and package splits harder to manage.

### Context and requirements
- Reset the Python namespace policy for workflow packages around maintainability and discoverability rather than around an artificially minimal facade.
- Keep package roots as curated convenience surfaces, but allow explicit public submodule imports for real domain concepts.
- Use underscore-prefixed modules only for true implementation helpers, low-level adapters, or private boundary plumbing.
- Allow breaking import-path changes where needed, and update all in-repo callers, tests, docs, and examples in the same change.
- Refactor `janiform.calibration`, `janiform.cli`, and `janiform.simulation` so large files can be split into coherent packages without creating underscore-named subpackages such as `_commands`.
- Revisit public API contract and snapshot tests so they verify the intended curated root exports plus the intended public domain submodules, instead of treating non-root importability as a policy failure.
- Keep the distinction between supported workflow/domain modules and truly internal helper modules explicit in docs and tests.

### Definition of Done
- Planning defines a replacement namespace policy for Python packages that permits public domain submodules and reserves underscore modules for true internals.
- `janiform.simulation` and `janiform.calibration` have a planned decomposition into named public submodules with curated root-level convenience re-exports where that improves ergonomics.
- `janiform.cli` has a planned decomposition that breaks up large command and parser modules into a structured package layout without forcing underscore-named subpackages.
- Public API snapshot, regression, and import-path tests are planned around the new policy and no longer encode the current accidental facade-only bias.
- Documentation and examples are planned to show both curated root imports and explicit submodule imports where the latter improve clarity.

### Agent implementation notes
- Do not preserve the existing namespace policy just because it is already documented. This project is specifically about replacing that accidental policy with one that scales.
- Apply breaking import-path changes directly. The repo is pre-v0.1. Update in-repo callers and docs directly.
- Prefer public submodules for coherent workflow areas such as simulation modes, calibration workflow layers, and CLI command groups.
- Keep package `__init__.py` files intentionally curated. They should provide convenience imports, not become the only discoverable map of a package.
- Avoid replacing one monolith with another. If a submodule is still too large after the first split, plan further decomposition into meaningful named modules.

### Initiative alignment
- Extends Initiative 006 from defining Python contracts to making the Python package layout sustainable enough to carry those contracts through v0.1 and beyond.
- Reduces the risk that v0.1 ships with technically documented APIs but an increasingly brittle Python package structure that discourages maintainable growth.
