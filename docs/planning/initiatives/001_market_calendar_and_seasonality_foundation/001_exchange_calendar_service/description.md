## Project 001: Exchange Calendar Service

### Context and requirements
- Build a canonical calendar component that resolves market state as `open` or `closed` for a timestamp and exchange.
- Support regular sessions, early closes, holidays, and weekend behavior.
- Provide deterministic boundary semantics at session open and close transitions.

### Definition of Done
- Rust core exposes a calendar domain model and query API used by downstream simulation and pricing paths.
- Python surface can query calendar state through a stable adapter.
- Unit tests cover normal sessions, holidays, early closes, and boundary timestamps.

### Agent implementation notes
- Keep calendar logic centralized to avoid duplicated session handling in separate modules.
- Use immutable calendar definitions loaded from versioned configuration artifacts.

### Initiative alignment
- Delivers the "canonical market calendar component" success signal for Initiative 001.
- Enables correct closed-window handling needed by `nu_news(t)` and gap return behavior.
