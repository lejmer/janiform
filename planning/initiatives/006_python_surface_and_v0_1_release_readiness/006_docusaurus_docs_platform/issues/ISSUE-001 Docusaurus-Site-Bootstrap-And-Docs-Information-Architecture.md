## ISSUE-001: Docusaurus Site Bootstrap And Docs Information Architecture

### Description
Create the Docusaurus site scaffold and establish the canonical documentation information architecture for Janiform. This issue should define the published site sections, navigation, sidebars, front-page goals, and the line between public docs, contributor docs, and non-published planning material.

### Acceptance Criteria
- A Docusaurus site is added to the repository with a documented local development and build workflow.
- Site structure includes clear top-level entrypoints for getting started, tutorials, guides, reference, concepts, architecture, and contributing.
- The repository has a documented rule for which content is published in the site versus kept in `planning/` or contributor-only areas.
- Root `README.md` is planned as a lightweight project entrypoint that links into the docs site instead of duplicating full documentation.
- Navigation is designed around target audiences and tasks, not around the current ad hoc folder layout.
- Contributor-facing instructions explain where docs source lives, how to run the docs site locally, and how to decide whether new material belongs in the published site, `README.md`, or `planning/`.

### Unit tests
- CI or local verification can build the Docusaurus site successfully.
- Validation exists for broken internal links and missing sidebar targets.

### Notes/Constraints
- Keep the initial site theme and customization modest. The first priority is architecture and readability.
- Prefer stable URLs that can survive content rewrites during the migration.
- Avoid inventing separate public sections for material that only exists because of current repository history.
- Treat developer instructions as part of the migration, not as follow-up cleanup.
