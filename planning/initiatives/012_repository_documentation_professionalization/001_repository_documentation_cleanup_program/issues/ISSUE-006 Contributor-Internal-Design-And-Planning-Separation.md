## ISSUE-006: Contributor, Internal Design, And Planning Separation

### Description
Separate contributor guidance, internal design records, and planning material so each surface serves its real purpose. Contributor docs should help engineers work in the repo, internal design docs should capture current technical rationale or reference material, and planning docs should remain execution-oriented records rather than accidental documentation surfaces.

### Acceptance Criteria
- Contributor docs are easy to find and contain development setup, testing, architecture navigation, and documentation contribution rules.
- Internal design material is organized as reference or decision support instead of being mixed into user onboarding flows.
- Brainstorming notes, milestone-specific plans, prompt templates, and issue records are clearly excluded from public docs.
- Cross-links between contributor docs, design records, and planning docs are intentional and labeled by purpose.
- Historical or superseded design material has an explicit archive or status treatment rather than remaining ambiguously current.

### Unit tests
- Docs validation ensures planning-only and contributor-only material is not included in the published docs surface.
- Navigation checks confirm contributor entrypoints link to the correct developer and architecture material.

### Notes/Constraints
- Preserve useful history, but do not let historical material masquerade as current guidance.
- Status labeling should be explicit when a design document is exploratory, active, superseded, or archived.
