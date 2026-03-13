## Project 001: Click-Based CLI Refactor

Replace the current `argparse` CLI with a Click-based application, reset the command tree around `fit` and `simulate`, migrate all supported command groups to the new structure, and remove superseded command names, parser code, tests, and temporary docs in the same change set.

### Context and requirements
- The current CLI has two competing models at the top level. It exposes high-level workflow verbs such as `fit` and `simulate`, while also exposing low-level workflow nouns such as `calibration` and `simulation`. That is structurally confusing and produces split-brain help output.
- The repo already contains the target direction in [docs/design/cli_redesign_rfc.md](../../../../docs/design/cli_redesign_rfc.md). This project is the execution vehicle for that redesign.
- The implementation should use Click, not continue extending `argparse`. Janiform now needs a real command-application framework with explicit groups, subcommands, shared option composition, consistent context handling, and maintainable help behavior.
- The command tree should be reset so:
  - `fit` remains the default high-level fitting workflow
  - `fit plan` replaces the current `calibration market-data` path
  - `fit staged` replaces the current `calibration staged` path
  - `simulate` remains the default high-level simulation workflow
  - `simulate coarse-interval` replaces `simulation coarse-interval`
  - `simulate seeded-ticks` replaces `simulation seeded-ticks`
- Standalone domain groups such as `presets`, `seasonality`, `hourly-factors`, `pricing`, `stylized-facts`, and `dev` should remain top-level because they are real standalone domains rather than variants of `fit` or `simulate`.
- The project must explicitly follow pre-v0.1 contract evolution rules. There are no external users yet. Do not add command aliases, backward-compatibility wrappers, deprecation warnings, migration guides, compatibility shims, or dual parser stacks. Replace the old CLI directly and update all in-repo callers, tests, examples, and temporary docs in the same refactor.
- Documentation in the repo is intentionally temporary. It should be updated enough that no stale command names or old parser architecture remain documented, but the project should not overinvest in polishing transitional prose that will be rewritten again later.
- The resulting plan should be self-contained. Engineers should be able to execute it without needing any additional conversational context.

### Definition of Done
- A project plan exists with focused issues for:
  - Click application foundation and shared CLI infrastructure
  - Click-native command boundary cleanup after the foundation lands
  - command-tree reset plus migration of `fit`, `simulate`, and their explicit low-level workflow subcommands
  - migration and cleanup of remaining command groups, help output, tests, examples, and temporary docs
  - final Click application simplification and removal of migration-era CLI structure once all commands live on the canonical tree
- The plan explicitly requires direct replacement of the old CLI surface and forbids compatibility shims, aliases, migration guides, and parallel `argparse` plus Click implementations.
- Each issue names the affected modules, target command paths, acceptance criteria, and test expectations tightly enough for direct implementation.
- The plan makes clear that temporary docs should be updated only far enough to remove stale names and architecture claims, not treated as the main deliverable of the refactor.
- The resulting project aligns with existing planning conventions and can be executed without hidden context.

### Agent implementation notes
- Keep the new CLI application thin at the business-logic boundary. Move command wiring to Click, not the underlying workflow behavior into CLI-specific abstractions.
- Prefer one canonical implementation per command path. Do not keep the old `argparse` implementation around under a legacy wrapper.
- Shared option behavior, output rendering, config precedence, and exit-code mapping should be implemented once and reused across command groups.
- Keep issue scope focused. This project is about the CLI application and command surface. Do not let it turn into a disguised redesign of unrelated workflow contracts unless a command migration truly requires it.
- Remove stale docs and examples aggressively enough that no engineer can mistake the old command tree for a supported path after the refactor lands.

### Initiative alignment
- This project is the first execution vehicle for turning the CLI redesign RFC into the actual pre-v0.1 Janiform command surface and for replacing improvised parser growth with a framework that can carry the CLI through v0.1.
