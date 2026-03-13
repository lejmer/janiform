## Click Native Command Boundary And Test Infrastructure Cleanup

### Description
Remove the temporary `argparse.Namespace`-style handler boundary and parser-emulation test adapter introduced during the Click foundation migration, so the remaining workflow migrations build on a canonical Click architecture rather than a compatibility layer.

JAN-240 establishes Click as the application framework, but the first pass keeps a small compatibility seam while the parser tree is being removed. That seam must not become the lasting design. Before migrating the `fit` and `simulate` workflow families and before finishing repository-wide cleanup, we need to convert the CLI boundary itself to normal Click patterns:

- command callbacks receive explicit typed parameters instead of a synthetic `Namespace`
- shared CLI state flows through a Click context object or other Click-native mechanism
- tests exercise the Click app directly instead of pretending there is still a parser object API

Without this issue, JAN-241 and JAN-242 risk re-implementing the old parser-era mental model inside Click rather than completing the migration.

### Implementation details
- Replace the synthetic `Namespace` handoff used by CLI command handlers with a Click-native boundary.
- Decide on one canonical boundary pattern for command handlers and use it consistently across the CLI:
  - direct callback parameters
  - a typed command-input dataclass built from Click parameters
  - a small shared app context object carried through `click.Context`
- Remove any parser-adapter layer whose only purpose is to let old parser-oriented tests keep running after the parser tree was deleted.
- Migrate shared helpers such as output emission, config precedence, and error rendering to consume the new Click-native boundary rather than `Namespace`.
- Update command tests and helper utilities to use Click-native invocation patterns, for example `CliRunner` or direct Click command invocation through the app entrypoint.
- Keep the business-logic boundary thin. This issue is about the CLI boundary shape, not about redesigning underlying workflow contracts.
- Make sure the result is simple enough that JAN-241 and JAN-242 can add or move commands without rebuilding another abstraction layer around Click.

### Acceptance Criteria
- No supported CLI production module imports `argparse.Namespace`.
- CLI command handlers no longer depend on a synthetic parser result object for attribute access.
- No compatibility parser adapter remains in the supported code path.
- CLI tests for command parsing, help, and routing use the Click application directly rather than a fake parser API.
- Shared CLI state and shared option behavior are expressed through one clear Click-native pattern that subsequent issues can reuse.
- JAN-241 and JAN-242 can proceed by wiring commands onto the new boundary rather than first undoing interim compatibility scaffolding.

### Unit tests
- Tests covering the chosen Click-native handler boundary for a representative set of commands.
- Regression tests for shared config precedence, output mode handling, and error mapping through the real Click app.
- Test-helper cleanup proving the CLI suite no longer relies on parser-emulation utilities.
- Entrypoint tests showing `janiform`, `python -m janiform`, and direct Click invocation all hit the same application surface.

### Notes/Constraints
- Do not keep a shim layer around just because it made JAN-240 easier to land. The purpose of this issue is to delete that shim.
- Keep the new boundary explicit and boring. Avoid clever decorator stacks or meta-framework code that obscures where Click parameters come from.
- This issue should land before JAN-241 and JAN-242, because those issues should migrate commands onto the final boundary, not onto a temporary bridge.
