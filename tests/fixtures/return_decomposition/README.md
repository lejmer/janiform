# Return Decomposition Sign Analytic Fixture

This fixture set stores compact deterministic analytic references for return
decomposition sign behavior.

## File

- `return_decomposition_sign_analytic_fixture.json`

## Contract

- `schema_version`: `return.decomposition.sign_analytic_fixture.v1`
- Fixture set id: `return_decomposition_sign_analytic_fixture_v1`
- Determinism metadata:
  - `fixture_loading_order`: `preserve_declared_case_order.v1`
  - `content_hash_scope`: `document_payload_bytes.v1`
  - `content_hash_algorithm`: `fnv1a_64_hex_utf8.v1`

## Scenario intents

- `buy_only`
- `sell_only`
- `combined`

Each case includes one `scenario_intent`, one explicit `return_decomposition`,
one explicit `gap_characteristic_factor`, and one `expected_output` payload.

## Expected output payload

- `buy_side_weighted_exponent`
- `sell_side_weighted_exponent`
- `combined_weighted_exponent`
- `trading_time_characteristic_factor`
- `combined_characteristic_factor`
