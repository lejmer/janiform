## Canonical Click Application Simplification And Migration-Era Trace Removal

### Description
Remove the remaining transitional Click structure that was acceptable while the CLI refactor was in flight but should not survive as the lasting pre-v0.1 architecture.

JAN-240 through JAN-243 establish the Click foundation and remove the temporary parser-shaped command boundary. JAN-241 and JAN-242 then migrate the supported command groups onto that new surface. After those issues land, Janiform should no longer carry code structure, helper shape, naming, or documentation language that exists only because the CLI used to be an `argparse` application.

This issue is the final cleanup pass that turns the refactor result into the canonical CLI architecture rather than a successful migration artifact. The target is not "good enough for now". The target is the cleanest command application we would design if Click had always been the framework.

There are no external users yet and no migration obligations. Do not preserve transitional structure, compatibility wrappers, dual naming, or explanatory scaffolding for the benefit of an audience that does not exist. Replace anything that still feels like a bridge with the final design directly.

This issue is not permission for JAN-241 or JAN-242 to land knowingly awkward command-group structure. Those issues should already implement the cleanest local design they can. JAN-244 exists for the residual simplification that only becomes obvious after the full command tree is live on Click and can be reviewed as one application.

### Implementation details
- Review the supported CLI modules under `python/janiform/cli/` and remove abstractions whose main value was easing the move from `argparse` to Click rather than serving the final application design.
- Settle on one explicit final command-definition pattern for the repo and apply it consistently across command groups. If the current command registration shape is more centralized or indirect than the final design needs, simplify it directly.
- Break up oversized command-wiring modules if they still act as migration-era assembly points rather than clear owners of the live Click command tree.
- Remove helper names, comments, doc language, test names, and internal symbols that still frame the CLI as a migration from an older parser architecture instead of simply the Janiform CLI.
- Re-evaluate shared option composition, app-context construction, and command registration helpers after JAN-241 and JAN-242 land:
  - keep helpers that provide real lasting value
  - delete or inline helpers that now exist only to preserve transitional indirection
- Keep one canonical implementation path per command and per shared policy. If multiple wrappers or assembly layers now reach the same live Click behavior, collapse them.
- Update contributor-facing CLI architecture docs so they describe the final command application shape without transitional framing such as "migration seam", "temporary bridge", "compatibility layer", "old parser mental model", or similar language that is no longer operationally relevant.
- Review CLI tests and test support for naming and structure that still reflects intermediate refactor steps rather than the final supported architecture.
- Keep the business-logic boundary thin. This issue is about deleting transitional CLI scaffolding, not redesigning calibration, simulation, pricing, or other domain workflows.

### Acceptance Criteria
- No supported CLI production module retains code whose primary purpose is to ease understanding of or compatibility with the former `argparse` architecture.
- The supported Click command tree is expressed through one clear final command-definition and registration pattern.
- No single CLI wiring module remains oversized purely because it accumulated transitional command registration during the refactor.
- Shared CLI helpers that remain in the supported path each have a clear lasting responsibility beyond migration convenience.
- Maintained CLI docs and contributor guidance describe the application as the canonical Janiform CLI, not as a recently migrated parser replacement.
- CLI test names, helpers, and structure no longer encode parser-era or transition-era assumptions.
- The resulting CLI is the shape we would keep into v0.1, not a compromise left over from landing the refactor incrementally.

### Unit tests
- Regression tests proving the final command-registration pattern still exposes the intended root commands and subcommands through the real Click app.
- Tests covering shared option behavior, config precedence, output handling, and error mapping after any simplification of helper layers.
- Test-support cleanup showing the CLI suite no longer depends on transitional helper names or intermediate command-assembly assumptions.
- Entrypoint and help-output tests confirming that architectural simplification does not change the supported command surface accidentally.

### Notes/Constraints
- This issue should land after JAN-241 and JAN-242. It is the pass that cleans up residual cross-cutting architecture once every surviving command group is already on Click.
- There are no users and no compatibility promises. Prefer direct replacement over preserving any transitional shape for continuity.
- Avoid clever framework code. If two possible designs are equally clean, choose the more explicit one.
- Do not turn this issue into unrelated workflow redesign. If the domain behavior is fine, leave it alone and simplify only the CLI architecture around it.
- Do not use this issue as a dumping ground for cleanup that was already clearly in scope for JAN-241 or JAN-242 when those issues were implemented.
