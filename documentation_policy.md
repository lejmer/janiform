# Janiform Documentation Policy

Version: `documentation_policy.v1`

This policy governs all tracked Markdown in the repository. It defines where documentation belongs, who it serves, and which document is authoritative when similar content appears in more than one place.

## Documentation Surfaces

| Surface | Audience | Canonical home | Purpose |
| --- | --- | --- | --- |
| `public` | Users, evaluators, external reviewers | `docs/` and root first-contact pages such as `README.md` | Installation, workflows, concepts, reference, and limitations for shipped behavior |
| `contributor` | Contributors and maintainers | `CONTRIBUTING.md`, `documentation_policy.md`, `documentation_style_guide.md`, language-specific contributor guides | How to change the repository safely and where to put documentation |
| `internal design` | Maintainers working through architecture and tradeoffs | Separate internal design records outside the user-facing docs surface | Design intent, alternatives, investigations, and rationale |
| `planning record` | Initiative owners and maintainers | Project and initiative tracking records | Scope, sequencing, issue tracking, planning ledgers, and execution planning |
| `generated` | Readers of generated reference material | Generated Markdown adjacent to the owning surface | Rendered output whose source of truth is code, schema, or structured data |
| `archive` | Readers who need historical context | An explicit `archive/` subtree inside the owning surface | Superseded material retained for traceability, not day-to-day guidance |

## Placement Rules

### Root Docs

The repository root is reserved for entrypoints and governance. Root Markdown should answer one of these needs:

- first contact for users or evaluators
- contributor process and repository rules
- release or changelog entrypoints

The root is not a catch-all location for subsystem notes, design records, or planning material.

### Published Docs

`docs/` is the canonical user-facing documentation surface. Public workflows, conceptual explanations, reference material, and user-facing examples belong there. Contributor process rules do not belong there.

Internal design material should live outside the user-facing docs surface. If a design rule becomes part of supported user behavior, promote that content into the appropriate public or contributor-facing document instead of sending readers to design notes.

### Local READMEs

Local `README.md` files explain the directory they live in. They should cover local purpose, inputs, outputs, maintenance notes, and links to canonical docs. They should not duplicate repository-wide onboarding, policy, or long workflow tutorials.

### Generated Markdown

Generated Markdown must preserve machine- or maintainer-readable provenance that identifies it as generated and points to the generator or source inputs. That provenance may live in rendered content, non-rendered comments, or adjacent generation metadata, depending on the surface. Contributors should edit the source of truth, then regenerate the Markdown, rather than patching generated output by hand.

## Canonical Source Rules

- One operational question gets one canonical answer. Other docs may summarize it briefly, then link to the owning page.
- `README.md` is a concise first-contact page. It links to deeper docs and does not try to be the full manual or contributor handbook.
- Public workflow behavior, commands, and interpretation guidance belong in `docs/`.
- Repository process, review expectations, and writing rules belong in the root governance files.
- Local READMEs own only local context. If a local README and a public guide start teaching the same workflow, the public guide becomes canonical and the README links to it.
- Planning records document intent and progress only. They do not override public docs, contracts, or contributor policy.
- Internal design notes explain rationale, not normative public behavior. If a design rule becomes an enforced contract, promote it into the appropriate canonical user or contributor doc.

## Stewardship And Review

- The author of a change owns the documentation updates required for that change.
- Public docs are reviewed by the feature owner or subsystem maintainer responsible for the behavior being documented.
- Local READMEs are owned by the directory or generator owner for that subtree.
- Planning records are owned by the initiative or project owner.
- Generated Markdown is owned by the maintainer of the generator and its source data.

### Documentation Review Checklist

- The document has a clear audience and lives on the correct surface.
- There is one canonical source for the topic, and duplicate wording was removed or reduced to a link.
- Commands, file paths, and examples were validated or clearly labeled as illustrative.
- Assumptions, limitations, determinism or reproducibility constraints, and provenance are stated when they matter.
- Related entrypoints, local READMEs, generated outputs, and replacement or archived pages were updated in the same change.

## Archive And Supersession

- Prefer updating the canonical source over leaving two competing pages active.
- When history must be retained, move the old document into an explicit archive surface or mark it with a visible supersession note that points to the replacement.
- Archived or superseded material must not remain linked from primary onboarding paths as if it were current guidance.
- Placeholder pages, empty stubs, and abandoned drafts should be removed instead of preserved as pseudo-documentation.

## Merge And Release Gates

Documentation updates are required when a change affects any of the following:

- installation, build, or test instructions
- public workflows, CLI commands, Python APIs, examples, or interpretation guidance
- directory ownership, generated Markdown workflows, or contributor process
- claims about determinism, provenance, limitations, or supported environments

A change is not release-ready until the relevant docs are updated and repository documentation checks pass.
