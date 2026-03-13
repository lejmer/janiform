## Calibration Package Public Submodule Decomposition

### Description
Refactor `janiform.calibration` into public domain submodules for high-level fitting, market-data workflows, staged execution, presets, and related calibration contracts, so the package can keep growing without concentrating nearly all Python calibration surface area into one giant `__init__.py`.

### Implementation details
- Replace the current underscore-heavy calibration module layout with public submodules for meaningful workflow areas such as fit, market-data, staged execution, presets, and session-resolution concerns where those are truly part of the supported surface.
- Keep low-level parsing, translation, or helper-only modules private when they are not useful public concepts on their own.
- Reassess which calibration symbols should stay in the root convenience surface versus which should be imported primarily from explicit submodules.
- Avoid duplicating contracts across root and submodules beyond intentional convenience re-exports.
- Split especially large calibration areas further when one public submodule would still be too large to maintain well.
- Update in-repo imports, tests, docs, examples, and any API-surface snapshots to the new package layout.

### Acceptance Criteria
- `janiform.calibration` has a documented decomposition into named public submodules that track actual workflow areas.
- Calibration root exports are curated and intentional rather than serving as the only discoverable map of the package.
- Underscore-prefixed calibration modules remain only where the code is truly helper or boundary plumbing.
- The old underscore-heavy layout is replaced directly instead of being preserved with parallel aliases.
- The resulting plan leaves room for future calibration growth without forcing underscore-named subpackages or ever-larger root re-export lists.

### Unit tests
- Refresh calibration public-surface tests and snapshots for the new import structure.
- Re-run calibration contract, market-data, preset, and high-level fit tests after the import-path refactor.
- Add import-path checks that cover the intended public calibration submodules.

### Notes/Constraints
- Keep the default-path high-level API planning from Project 008 compatible with this decomposition. The high-level surface may stay root-friendly while the underlying package structure becomes more explicit and maintainable.
- Reuse one canonical implementation per workflow area. Do not preserve duplicate modules just to keep both old and new names alive.
