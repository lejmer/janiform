## CSV And Parquet Market Data Ingestion And Column Mapping

### Description
Add the ingestion and normalization planning needed to load `csv` and `parquet` market data sources, validate their shape, and map them into Janiform's canonical calibration input vocabulary.

### Implementation details
- Define loader policies for `csv` and `parquet` tick-data inputs and optional option-surface inputs.
- Define explicit column-mapping rules for timestamps, prices, bid, ask, trade price, close price, and other required fields where applicable.
- Specify normalization policy for timestamp parsing, timezone handling, sort order, duplicate handling, missing-data behavior, and price derivation such as midpoint from bid and ask.
- Define override hooks when file schema inference is ambiguous or when multiple price interpretations are possible.
- Define file-format and schema validation error taxonomy with actionable messages for market users.
- Ensure ingestion planning can preserve source-file provenance and row-count or filtering summaries for downstream artifacts and diagnostics.

### Acceptance Criteria
- Planning supports both `csv` and `parquet` as first-class source formats for user-facing calibration workflows.
- Column-mapping and normalization policy is explicit enough to support deterministic preprocessing.
- Ambiguous or unsupported file layouts fail with actionable validation guidance instead of silent fallback behavior.
- Ingestion outputs expose enough provenance and summary metadata for downstream reporting and triage.

### Unit tests
- Loader and schema-validation tests for representative `csv` and `parquet` fixtures.
- Column-mapping tests for bid and ask midpoint derivation, close-only inputs, and malformed timestamp fields.
- Deterministic ordering and duplicate-handling checks on fixed fixtures.
- Validation coverage for unsupported file formats and incomplete mappings.

### Notes/Constraints
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
- Keep schema inference bounded and explicit. Users should be able to override it deterministically.
- Avoid pushing file-ingestion policy into the staged calibration engine or PyO3 boundary.
