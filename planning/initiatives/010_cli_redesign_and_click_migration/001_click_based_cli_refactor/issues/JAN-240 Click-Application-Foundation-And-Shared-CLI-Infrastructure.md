## Click Application Foundation And Shared CLI Infrastructure

### Description
Replace the current `argparse` parser-construction and dispatch scaffolding with a Click-native application foundation that supports the redesigned command tree, shared option policies, consistent output behavior, and maintainable future command growth.

The current CLI already behaves like a framework layered on top of `argparse`. Parser construction is spread across `python/janiform/cli/parsing/*.py`, dispatch is routed manually, and shared concerns such as output mode, config loading, path resolution, and error handling are stitched through parser and command modules by hand. That was acceptable while the CLI was small. It is no longer a good foundation for the refactor.

This issue is not just about swapping one parser library for another. It is about establishing the canonical Click application structure that every migrated command group will use.

### Implementation details
- Define the Click root application and command-group ownership structure under `python/janiform/cli/` so command registration mirrors the intended command tree rather than the current parser buckets.
- Replace parser-construction modules such as `python/janiform/cli/parsing/command_tree.py`, `python/janiform/cli/parsing/configuration.py`, and the per-group parsing modules with Click command and group definitions.
- Establish one shared mechanism for:
  - output mode handling
  - config-file loading and precedence
  - environment-variable defaults
  - repository-root path resolution
  - structured error rendering and exit-code mapping
  - common command context that Click subcommands can consume without rebuilding the same plumbing
- Keep business logic in existing workflow modules where possible. The Click layer should own CLI boundary concerns, not duplicate workflow implementations.
- Decide how shared options are composed across command groups. Avoid ad hoc decorator stacks that become harder to reason about than the current `argparse` helper functions.
- Ensure the root `janiform` script entrypoint and `python -m janiform` flow still resolve through one thin application entrypoint.
- Remove the old `argparse` scaffolding directly once the Click foundation is in place. Do not leave a parallel legacy parser tree behind.

### Acceptance Criteria
- The canonical CLI application is implemented with Click rather than `argparse`.
- Shared CLI concerns are centralized into reusable Click-native helpers or context objects instead of being spread across parser-construction modules.
- The root entrypoint remains thin and readable after the migration.
- The old parser-construction tree is removed rather than preserved in parallel.
- The resulting application structure is clear enough that subsequent command-group migrations do not need to invent a second command-registration pattern.

### Unit tests
- Smoke tests for root command invocation, version output, and top-level help through the new Click application.
- Tests for shared output-mode behavior, config precedence, environment overrides, and path resolution under the Click context.
- Error-handling tests showing validation, runtime, and internal failures still map to the intended exit codes and structured stderr output.
- Regression tests confirming `python -m janiform` and the console-script entrypoint invoke the same application surface.

### Notes/Constraints
- This issue must not preserve a fallback `argparse` path. Pre-v0.1 rules require direct replacement.
- Keep the Click app explicit. The goal is maintainable command architecture, not magic that hides where commands and shared options come from.
