## Project 003: Return Decomposition And Sign Conventions

Standardize return composition and characteristic-function sign conventions across pricing and simulation paths.

### Context and requirements
- Implement return composition `X = L_plus(A_plus) - L_minus(A_minus) + G` with strict sign handling.
- Ensure characteristic-function composition applies sell-side exponent with `-u` argument.
- Keep decomposition API consistent between simulation and pricing code paths.

### Definition of Done
- Shared return composition module exists and is imported by both engines.
- Characteristic-function assembly uses validated sign conventions with targeted regression tests.
- Tests cover edge cases where one component is disabled to isolate sign and composition correctness.

### Agent implementation notes
- Keep interfaces explicit for `A_plus`, `A_minus`, and gap contribution to avoid accidental swaps.
- Add focused golden tests derived from simplified analytic cases.

### Initiative alignment
- Directly fulfills Initiative 003 success signal on sell-side sign convention and component reuse.
- Reduces pricing and simulation divergence risk.
