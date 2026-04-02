## Public Workflow Docs Rewrite And Migration

### Description
Rewrite the user-facing workflow documentation so it guides external users from installation to successful use of Janiform without exposing them to internal contract sprawl prematurely. The primary sequence should cover install, first successful market-data calibration, simulation from a fitted model, pricing, output interpretation, and known limitations or assumptions.

### Acceptance Criteria
- Public docs contain a coherent onboarding path for installation, quickstart, market-data-first calibration, simulation, and pricing.
- The rewritten workflow pages live under the public docs tree in explicit homes: installation and quickstart under `docs/getting-started/`, user journeys under `docs/workflows/`, supporting explanations under `docs/concepts/`, and detailed command or API material under `docs/reference/`.
- Workflow pages follow a consistent structure covering purpose, prerequisites, minimal inputs, executable examples, expected outputs, and failure modes.
- Deep internal contract or design documents are summarized or linked only where they add real value for advanced users.
- Existing user-facing content has explicit migration outcomes recorded in the ledger rather than being mechanically copied.
- Public docs reflect current supported CLI and Python surfaces and do not rely on stale milestone-specific behavior notes.
- Workflow pages explain assumptions, limitations, and interpretation guidance where those details materially affect financial use.
- The workflow set includes at least one end-to-end path from raw market data to fitted-model outputs that a new user can actually reproduce.

### Unit tests
- Public workflow examples are validated through smoke checks, generated examples, or documentation tests where practical.
- Link checks pass across all migrated workflow pages.
- At least one docs verification target exercises the primary user journey end to end rather than only isolated commands.

### Notes/Constraints
- Market-data-first calibration is the primary onboarding path.
- Prefer fewer strong workflow pages over many thin placeholders.
- Root files such as `README.md` may summarize and link these workflows, but the canonical workflow content lives in `docs/`, not in root Markdown.
