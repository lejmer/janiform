## Initiative 010: CLI Redesign And Click Migration

Replace the current split-brain `argparse` CLI with a production-ready Click-based command application that presents `fit` and `simulate` as the main user workflows, nests lower-level control under those same roots, and removes obsolete command names and temporary command-layout assumptions before v0.1 freezes them into the public surface.

### Desired outcome
- Janiform exposes one coherent CLI mental model instead of competing top-level concepts such as `fit` versus `calibration` and `simulate` versus `simulation`.
- The CLI is implemented on top of Click rather than continued manual `argparse` plumbing, so command groups, shared options, help behavior, shell ergonomics, and future command growth have a proper application framework.
- `janiform --help` clearly teaches first-time users to start with `fit` and `simulate`, while still preserving direct access to explicit lower-level workflows where that control is actually needed.
- The refactor directly removes obsolete command paths, parser code, tests, docs, and examples instead of preserving compatibility shims, aliases, migration scaffolding, or other pre-v0.1 bloat.
- Planning for the refactor is implementation-ready and explicit enough that an engineer can execute it without relying on conversational context from the CLI redesign discussion.

### Success signals
- The command tree no longer exposes both high-level and low-level versions of the same concept as separate top-level roots.
- `argparse` parser-construction modules and manual dispatch scaffolding are replaced by a Click-native application structure with clear command-group ownership.
- The new top-level help output makes `fit` and `simulate` the obvious entry points and no longer teaches superseded roots such as `calibration`, `simulation`, or `fit-and-simulate`.
- In-repo tests, examples, and working docs use only the new command tree and do not preserve the old names "just in case".
- The planning explicitly states that there are no external users yet, so the implementation does not waste effort on compatibility layers, migration guides, deprecation periods, or dual command-path support.

### Example projects that might be useful to add
- A project focused on the Click application foundation, shared command infrastructure, and high-level workflow migration.
- A project focused on richer shell completion, terminal styling, and CLI operator ergonomics after the command-tree reset is complete.
- A project focused on packaging and release-hardening for the final pre-v0.1 CLI distribution once the Click refactor has settled.
