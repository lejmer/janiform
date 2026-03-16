## Canonical Microbenchmark Tooling For Rust And Python Narrow Paths

### Description
Add a canonical microbenchmark layer for the narrower performance use cases in this repository so that we keep the current fixture-driven acceptance benchmarks for end-to-end governance while also adopting the standard tooling that Rust and Python users expect for lower-level timing work.

The repository now has custom benchmark infrastructure for workflow-scale performance governance:
- Rust benchmark harnesses and accepted baselines for oracle, pricing, and risk paths
- a fixture-driven real-data seeded runtime benchmark for fit plus simulation plus Python boundary behavior
- scheduled CI wiring that compares machine-readable benchmark artifacts against checked-in acceptance baselines

That infrastructure is solving an important problem, but it is not the canonical way engineers usually benchmark narrower code paths in Rust plus Python-wrapper projects. The current setup is optimized for policy, provenance, baseline comparison, and failure classification rather than for ergonomic local microbenchmark iteration.

This gap matters in practice:
- there is no `Criterion` benchmark suite for pure Rust hot paths or engine kernels
- there is no canonical Python microbenchmark suite for wrapper overhead, materialization patterns, or thin workflow entrypoints
- contributors who want to answer questions like “did this Rust kernel get faster” or “did this Python accessor add per-call overhead” have to use the higher-level custom harnesses, which are heavier and answer a different class of question
- the repository therefore lacks the standard performance workflow most contributors will expect in a Rust plus Python project

This issue is specifically about adding that second benchmark layer, not about replacing the current custom benchmark and baseline machinery. The intended end state is hybrid:
- keep the current custom benchmark system for end-to-end acceptance governance
- add canonical tooling for low-level or narrower timing-oriented benchmarks

### Implementation Details
- Add `Criterion`-based Rust benchmark targets under `crates/core/benches/` for representative pure-Rust kernels and component-level hot paths.
- Start with a bounded but meaningful set of Rust benchmarks, such as:
  - oracle solver inner-path work
  - pricing kernel or transform-evaluation hot paths
  - risk sampling or sufficient-statistics kernels
  - simulation primitives that are meaningful without the Python boundary
- Define benchmark input builders or fixtures for those Criterion benches so they use deterministic, checked-in inputs rather than ad hoc inline constants where realistic setup matters.
- Add a canonical Python microbenchmark suite using `pytest-benchmark` for narrower Python-facing paths such as:
  - thin wrapper overhead around native calls
  - seeded result accessors and representative materialization patterns
  - serialization or conversion paths where Python overhead is itself the unit under test
  - high-level API entrypoints where the benchmark goal is interface overhead rather than full workflow acceptance governance
- Keep Python microbenchmarks scoped and narrow. Do not duplicate the existing fixture-driven acceptance benchmarks for end-to-end workflow governance.
- Add benchmark organization and naming guidance so contributors know when to use:
  - Criterion
  - pytest-benchmark
  - the existing fixture-driven custom benchmark and baseline system
- Document the benchmark taxonomy explicitly:
  - microbenchmark
  - component benchmark
  - workflow acceptance benchmark
  - CI governance benchmark
- Add contributor documentation for running the canonical benchmark suites locally, including when benchmark results are informative and when they are too noisy for CI gating.
- Add a lightweight CI or manual benchmark workflow for the canonical benchmark suites if practical, but do not make noisy microbenchmarks required merge gates by default.
- Ensure the canonical benchmark tooling can coexist cleanly with the existing benchmark artifacts and scheduled acceptance benchmark workflows rather than competing with them.
- Reuse existing deterministic fixture builders and bounded representative data where possible instead of inventing a second set of unrelated benchmark inputs.
- Be explicit about benchmark semantics:
  - Criterion and pytest-benchmark are for local iteration and narrower timing signals
  - the existing fixture-driven baseline system remains the source of truth for end-to-end acceptance checks and regression classification

### Acceptance Criteria
- The repository includes `Criterion` benchmarks for a representative set of pure-Rust hot paths under `crates/core/benches/`.
- The repository includes a canonical Python microbenchmark suite for narrower wrapper or boundary paths using `pytest-benchmark`.
- Benchmark documentation explains clearly which benchmark mechanism should be used for:
  - low-level Rust timing work
  - Python-wrapper or accessor timing work
  - workflow-scale acceptance benchmarking
- The canonical benchmark suites are runnable locally with documented commands and bounded representative inputs.
- The canonical benchmark suites do not replace or weaken the existing fixture-driven acceptance benchmark and baseline system.
- CI or manual workflow wiring exists for the canonical benchmark suites at an appropriate tier that does not introduce unstable required gates on every commit.
- At least one benchmark exists for each of the following narrower categories:
  - pure Rust kernel performance
  - Python boundary or accessor overhead
  - Python serialization or materialization overhead
- The repository docs explain why the project uses both:
  - canonical tooling for microbenchmarks
  - custom fixture-driven benchmark governance for workflow-scale acceptance

### Unit tests
- Smoke tests asserting the Criterion benchmark targets are wired and discoverable.
- Smoke tests asserting the pytest-benchmark suite is wired and runnable in the repository test environment.
- Documentation or command-reference tests asserting benchmark commands and benchmark taxonomy guidance stay current.
- Regression tests for any deterministic benchmark input builders or shared fixtures introduced for the canonical benchmark suites.

### Notes/Constraints
- Do not remove or redesign the existing custom benchmark and baseline machinery in this issue.
- Keep the first Criterion and pytest-benchmark suites bounded and focused on narrower use cases rather than trying to benchmark every code path immediately.
- Avoid turning noisy microbenchmark results into hard required merge gates unless stability is demonstrated first.
- Prefer deterministic and representative benchmark inputs over synthetic but meaningless microbench constants when realistic setup materially affects performance.
- Keep terminology precise so contributors understand that a microbenchmark timing regression is not the same thing as a workflow acceptance benchmark failure.
