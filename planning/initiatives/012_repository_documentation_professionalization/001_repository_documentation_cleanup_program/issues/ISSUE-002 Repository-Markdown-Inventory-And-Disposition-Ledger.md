## ISSUE-002: Repository Markdown Inventory And Disposition Ledger

### Description
Build a complete ledger of tracked Markdown files and assign a disposition for each file. The ledger should include file path, audience, owner, current role, target role, canonical status, and migration decision such as keep, rewrite, merge, move, archive, generate, or delete.

### Acceptance Criteria
- Every tracked `.md` file in the repository appears in the ledger.
- The ledger records whether the file is public-facing, contributor-facing, internal-only, planning-only, generated, or archival.
- The ledger identifies empty, obsolete, duplicated, milestone-specific, or trust-eroding files that should be removed or replaced.
- The ledger identifies migration targets for content that should move into published docs, contributor docs, or archive locations.
- The ledger is linked from the documentation policy or project docs so future cleanup work has one canonical tracking artifact.

### Unit tests
- A repository validation step compares tracked Markdown files against the ledger and fails when a file is missing from the inventory.
- Validation checks that every ledger entry has a disposition and target category.

### Notes/Constraints
- The goal is decision-quality inventory, not prose polish.
- Generated files may be represented by their source-of-truth workflow if that makes the ledger clearer.
