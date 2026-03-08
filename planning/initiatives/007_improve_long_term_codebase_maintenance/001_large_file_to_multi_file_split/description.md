## Project 001: Large File To Multi-File Split

Isolate large files that are hard to maintain and split these into multiple files inside one folder per split file.

### Context and requirements
- Isolate top-level files that are hard to maintain.
- Create folders, one per large top-level file, and split each file into many, into their respective folder, with a `mod.rs` in each.

### Definition of Done
- No more giant top-level files exist.
- All external import paths stay the same.
- All code still compiles.
- All tests still pass.

### Initiative alignment
- We have no more giant top-level files, and it is easier to navigate the codebase.
