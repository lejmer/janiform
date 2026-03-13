## CLI Package Decomposition And Command Group Architecture

### Description
Break `janiform.cli` out of the current large `_commands` and parser modules into a maintainable package structure organized around command groups, parsing, rendering, and shared CLI boundary concerns.

### Implementation details
- Replace `_commands.py` as the primary CLI implementation bucket with a package layout that can support multiple command-group modules without requiring an underscore-named subpackage.
- Separate command dispatch, argument parsing, output rendering, environment and path resolution, and workflow adapters into clearer modules.
- Do not give any of the new modules an underscored named.
- Preserve a simple `janiform.cli.main` entrypoint and the `python -m janiform` flow while allowing the internal package structure to grow sanely.
- Decide which CLI modules should remain internal helpers and which should be named modules because they represent real command-group organization.
- Update docs and tests that currently import `_commands` or `_parser` directly when those internals are replaced.
- Please look at python/janiform/calibration/ and python/janiform/simulation/ as an example for how we

### Acceptance Criteria
- The CLI package no longer centers around a monolithic `_commands.py` file.
- The package layout supports further command growth without forcing underscore-named subpackages such as `_commands.*`.
- CLI main entrypoint behavior remains clear and thin after decomposition.
- In-repo tests and helpers stop depending on old monolithic module names unless a dependency is deliberately retained as a true internal helper.
- The planned structure distinguishes command-group modules from low-level helper modules.

### Unit tests
- Re-run CLI workflow, parser, smoke, and command-reference tests after decomposition.
- Add focused tests for command-group routing and parser construction in the new module structure.
- Add import-path checks for the intended CLI package layout where useful.

### Notes/Constraints
- CLI internals do not need to be flattened into the root package just because `main()` remains the primary entrypoint.
- Prefer decomposition that mirrors the command tree itself, not one generic utility bucket per technical mechanism.
