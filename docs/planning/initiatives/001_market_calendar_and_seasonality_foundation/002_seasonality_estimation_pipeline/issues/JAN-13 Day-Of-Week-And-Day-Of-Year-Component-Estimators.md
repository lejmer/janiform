## Day Of Week And Day Of Year Component Estimators

### Description
Implement nonparametric estimators for `f_dow(dow)` and `f_doy(doy)` and compose them with intraday structure into a unified deterministic seasonality signal.

### Acceptance Criteria
- Day-of-week estimator captures weekday effects from event timestamp data.
- Day-of-year estimator captures seasonal and holiday-period structure at daily resolution.
- Component composition is explicit, deterministic, and documented.
- Missing or sparse buckets are handled with defined fallback behavior.

### Unit tests
- Tests for synthetic weekly patterns that recover expected day-of-week effects.
- Tests for yearly seasonal patterns and sparse day buckets.
- Composition tests confirming deterministic combined output.

### Notes/Constraints
- Keep component estimators separable so they can be tuned independently.
- Reuse shared preprocessing outputs to avoid duplicated transformations.
