## Split Python Files To Multiple Files

### Description

Split `calibration.py` and `cli.py` into multiple files. Turn them into one package each, re-exporting via `__init__.py` to preserve the existing import paths. Split them into modules that make sense.

### Acceptance Criteria

- Both files are split into multiple files.
- All tests still pass.
