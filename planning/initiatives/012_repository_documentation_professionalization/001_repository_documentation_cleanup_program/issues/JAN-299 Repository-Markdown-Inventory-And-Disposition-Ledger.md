## Repository Markdown Inventory And Disposition Ledger

### Description
Build a complete ledger of tracked Markdown files and assign a disposition for each file. The ledger should include file path, audience, owner, current role, target role, canonical status, and migration decision such as keep, rewrite, merge, move, archive, generate, or delete.

### Acceptance Criteria
- The canonical ledger lives with this project at `planning/initiatives/012_repository_documentation_professionalization/001_repository_documentation_cleanup_program/markdown_inventory_and_disposition_ledger.md`.
- Every tracked `.md` file in the repository appears in the ledger.
- The ledger records whether the file is public-facing, contributor-facing, internal-only, planning-only, generated, or archival.
- The ledger identifies empty, obsolete, duplicated, milestone-specific, or trust-eroding files that should be removed or replaced.
- The ledger identifies migration targets for content that should move into published docs, contributor docs, or archive locations.
- The ledger is linked from the documentation policy or project docs so future cleanup work has one canonical tracking artifact.
- The ledger records the current source of truth when a Markdown file is generated from code or scripts.

### Validation
- Before closing the issue, compare the ledger against `git ls-files '*.md'` and confirm that every tracked Markdown file is represented.
- Spot-check that every ledger row has a disposition, target surface, and source-of-truth note where relevant.

### Notes/Constraints
- The goal is decision-quality inventory, not prose polish.
- Keep the ledger easy to diff and maintain during follow-on cleanup work.
- Keep the ledger in planning rather than at repository root because it is a cleanup-program control document, not a permanent public or contributor-facing policy page.
- The unit tests added here will be deleted later. They only server as validation in this Issue.
