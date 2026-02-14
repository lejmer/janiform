## Project 001: Hawkes State Kernel

### Context and requirements
- Implement the 4D Hawkes excitation state and transition equations with shared decay.
- Support event-driven updates for buy and sell arrivals and exogenous news loading.
- Keep state evolution reusable for fine-scale simulation and coarse-time transform code.

### Definition of Done
- Core module exposes deterministic state transition functions for drift and jump updates.
- One canonical implementation is used across simulation, calibration, and transform paths.
- Tests verify equation-level correctness with seeded deterministic scenarios.

### Agent implementation notes
- Keep state representation explicit and typed to avoid index-order bugs.
- Avoid duplicating transition logic in separate engines.

### Initiative alignment
- Delivers the Initiative 002 success signal that 4D state and update rules are implemented once.
- Establishes the canonical fine-scale model substrate for all event generation.
