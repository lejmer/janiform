# Janiform Documentation Style Guide

Version: `documentation_style_guide.v1`

This guide defines how Janiform documentation should read. Use it for root docs, published docs, local READMEs, and contributor guidance unless a generated file has stricter template rules.

## Tone

- Write for technically literate readers, including market practitioners, open-source adopters, and maintainers.
- Be direct, specific, and professional. Avoid hype, filler, and sales language.
- Separate current behavior from future intent. Do not present planned work as shipped capability.
- Prefer precise claims over broad claims. If a statement depends on scope, state the scope.

## Structure

- Start with the purpose of the page and the reader it serves.
- Put task flow before implementation history.
- Use short sections with descriptive headings.
- Keep examples complete enough to run or clearly mark them as schematic.
- Call out platform-specific steps when Windows and Linux differ.
- Link outward instead of copying the same explanation into multiple places.

## Terminology

- Use one term for one concept across the repository.
- Prefer the names used by the public API, CLI, contracts, and fixture schemas.
- Expand acronyms on first use unless the audience is already constrained by the local context.
- Distinguish documentation surfaces explicitly when needed: `public`, `contributor`, `internal design`, `planning record`, `generated`, and `archive`.

## Status Labels

Use a visible status line when omission would create ambiguity. Keep the label short and factual.

- `Status: Canonical` for the owning page of an active topic
- `Status: Generated` for generated Markdown
- `Status: Internal design` for architecture or investigation material
- `Status: Planning record` for initiative, project, or issue tracking material
- `Status: Archived` for retained historical material
- `Status: Superseded by <path>` when a document is kept only for backward traceability

## Required Context

Add the following context when it affects interpretation:

- `Assumptions`: dataset shape, market conventions, time zone handling, environment expectations, or prerequisite tooling
- `Limitations`: unsupported cases, intentional exclusions, or boundaries of validation
- `Determinism` or `Reproducibility`: whether the same inputs should produce identical outputs, and which inputs, seeds, artifacts, or environment details control that outcome
- `Provenance`: where examples, benchmark claims, fixtures, schemas, or generated content came from

Do not add these sections mechanically. Add them when they change how a reader should trust or use the document.

## Writing Rules

- Prefer active voice.
- Use exact commands, file paths, and identifiers.
- Replace vague references such as "here", "above", and "recently" with concrete names.
- Do not leave TODO-style placeholders in committed user or contributor docs.
- If a document is not the canonical source, say so briefly and link to the owner.
