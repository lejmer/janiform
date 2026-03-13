## Command Tree Reset And Fit Simulate Workflow Migration To Click

### Description
Reset the CLI command tree around `fit` and `simulate`, then migrate those workflow families and their lower-level subcommands onto the new Click application structure so the public surface and implementation change together.

This issue assumes JAN-243 has already removed the temporary `Namespace`-style handler boundary and parser-emulation test layer from the Click foundation. The commands migrated here should land directly on that final Click-native boundary rather than extending any interim compatibility scaffolding.

This issue covers the commands most users should actually touch and the command-path changes that make them coherent:

- `janiform fit`
- `janiform fit plan`
- `janiform fit staged`
- `janiform simulate`
- `janiform simulate coarse-interval`
- `janiform simulate seeded-ticks`

It also owns the removal of the old duplicate top-level roots:

- `janiform calibration`
- `janiform simulation`
- `janiform fit-and-simulate`

If the combined fit-plus-simulate workflow remains supported, it should be represented as a mode of `fit`, not as its own root.

### Implementation details
- Implement the canonical command-tree structure for this part of the CLI:
  - `fit`
  - `fit plan`
  - `fit staged`
  - `simulate`
  - `simulate coarse-interval`
  - `simulate seeded-ticks`
- Rebuild the current high-level fit workflow on Click under `fit`.
- Migrate `calibration market-data` semantics to `fit plan`, using a name and help text that explain the command in user-facing terms rather than internal calibration-boundary jargon.
- Migrate `calibration staged` semantics to `fit staged`.
- Rebuild the current high-level model-driven simulation workflow on Click under `simulate`.
- Migrate `simulation coarse-interval` to `simulate coarse-interval`.
- Migrate `simulation seeded-ticks` to `simulate seeded-ticks`.
- Remove the superseded top-level roots directly instead of leaving compatibility stubs behind.
- Decide how the current fit-plus-simulate convenience path is represented after the redesign:
  - remove it completely, or
  - fold it into `fit` as explicit simulation flags
- Keep one canonical implementation path for each workflow. Do not preserve the old command functions under renamed wrappers if the new Click command can call the same underlying workflow logic directly.
- Use the Click-native command boundary established by JAN-243. Do not introduce new synthetic parser-result objects, `Namespace` adapters, or parser-emulation helpers while migrating these workflows.
- Ensure top-level help presents `fit` and `simulate` as the primary entry points, and ensure `fit --help` and `simulate --help` make the default workflow explicit while framing subcommands as more explicit control levels rather than unrelated sibling commands.
- Review command constants, output payload `command` identifiers, tests, examples, and temporary docs touched by these workflow families so they all reflect the new canonical paths.

### Acceptance Criteria
- The CLI no longer exposes top-level `calibration`, `simulation`, or `fit-and-simulate` command paths.
- `fit` and `simulate` work as Click commands with the intended default behavior.
- `fit plan`, `fit staged`, `simulate coarse-interval`, and `simulate seeded-ticks` are wired to the correct underlying workflow logic.
- `fit` and `simulate` are the obvious primary entry points in top-level help.
- Help text for `fit` and `simulate` clearly communicates the default workflow and the role of each subcommand.
- The migrated workflows preserve current validation, output, and exit-code behavior unless the redesign intentionally changes the public CLI semantics.
- The migrated workflows sit directly on the Click-native boundary from JAN-243 rather than on temporary parser-compatibility seams.
- No in-repo test, example, or maintained temporary doc still presents the removed roots as supported for these workflow families.

### Unit tests
- Help-output regression tests for `janiform --help`, `janiform fit --help`, and `janiform simulate --help`.
- High-level workflow tests for `fit` and `simulate`.
- Command-specific tests for `fit plan`, `fit staged`, `simulate coarse-interval`, and `simulate seeded-ticks`.
- Tests covering any retained combined fit-plus-simulate mode if that mode remains supported under `fit`.
- Negative tests confirming the removed command roots are no longer accepted.
- Regression tests for output payload structure, command labels, and human-readable summaries on the migrated commands.

### Notes/Constraints
- Keep the combined fit-plus-simulate behavior subordinate to `fit` if it survives at all. It must not remain a top-level peer command.
- There are no users yet. Do not add aliases, deprecation messages, or compatibility command stubs for the removed roots.
- If JAN-243 has not landed yet, this issue is blocked. It should not absorb the command-boundary cleanup work indirectly.
- This issue should not re-litigate the command-tree design. Its job is to implement the agreed model cleanly.
