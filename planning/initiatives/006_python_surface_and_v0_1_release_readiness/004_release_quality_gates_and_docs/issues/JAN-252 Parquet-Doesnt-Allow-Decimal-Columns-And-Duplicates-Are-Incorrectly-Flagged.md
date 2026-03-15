## Parquet Doesn't Allow Decimal Columns And Duplicates Are Incorrectly Flagged As Errors

### Description
Currently, .parquet import validation does not support decimal columns. They should. Duplicate trade are now treated as bad input. Real data has duplicate trades, and removing them would bias the arrival rate for the Hawkes process. They should be allowed.

### Acceptance Criteria
- Decimal columns are allowed in imported .parquet files and no longer cause an error.
- Duplicate trade ticks are allowed and no logic breaks as a result of allowing them.
