# Workflow Module Conventions

Fit, pricing, and simulation workflows follow one ownership vocabulary inside the Rust workflow
and PyO3 boundary crates.

## Rust Workflow Crate

| Module | Owns |
| --- | --- |
| `request.rs` | Rust workflow request contracts and filesystem/path request shapes |
| `workflow.rs` | Observer contracts and cross-entrypoint workflow coordination types |
| `output.rs` | Workflow output contracts and start/completion summary payloads |
| `execution.rs` | Observer-aware workflow coordination and lower-level execution entrypoints |
| `run.rs` | Terminal convenience entrypoints that construct the standard observer |
| `events.rs` or `progress.rs` | Domain workflow events and terminal progress adaptation |

## PyO3 Boundary Crate

The PyO3 boundary mirrors this shape with `request.rs`, `events.rs`, `output.rs`, and `workflow.rs`
modules. PyO3 request payloads are named with their workflow domain and variant, for example
`FitPreparedRunRequestPayload` or `PricingActivityCompressedScheduleRequestPayload`.

Boundary payloads use domain-specific names because they are not public workflow contracts.
