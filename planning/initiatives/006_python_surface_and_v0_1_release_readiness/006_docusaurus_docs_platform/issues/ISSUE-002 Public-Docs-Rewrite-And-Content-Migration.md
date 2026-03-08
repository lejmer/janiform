## ISSUE-002: Public Docs Rewrite And Content Migration

### Description
Rewrite the current user-facing documentation into production-ready docs pages inside Docusaurus. Every significant existing document should be classified as rewrite, merge, contributor-only, internal-only, or archive, and the migration should prioritize a coherent end-user experience over preserving one-file-per-topic history.

### Acceptance Criteria
- The root `README.md` is rewritten into a production-ready repository entrypoint with concise project overview, install path, quickstart, docs links, and contribution links.
- The site contains production-quality pages for installation, quickstart, first successful simulation, first successful pricing run, and first successful market-data calibration run.
- The site contains conceptual pages for the model, system architecture, and the Rust/Python boundary at a level appropriate for external users and contributors.
- Existing docs content has a migration ledger that records source files and their destination status: rewrite, merge, move, or archive.
- Public docs no longer expose brainstorming notes, planning records, prompt templates, or milestone-specific status text as first-class published pages.
- `README.md` and published docs no longer duplicate long command inventories or giant index lists of internal specs.
- Contributor-visible docs explain the new documentation structure well enough that future docs work follows the same separation of concerns.

### Unit tests
- Link checks pass across migrated pages.
- Example snippets referenced by onboarding and workflow pages are validated through smoke checks or script-backed documentation tests where practical.

### Notes/Constraints
- Rewrite for audience and clarity instead of mechanically porting current Markdown into Docusaurus.
- Prefer fewer strong pages over preserving every current document as an independent page.
- Market-data-first calibration must remain a primary user journey, not an appendix.
- Keep `README.md` intentionally smaller than the site and treat it as a gateway rather than the canonical full docs surface.
