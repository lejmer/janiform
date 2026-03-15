## Split Loading And Translating Step Into Individual Steps

### Description
Currently in the high-level workflow, we have one big step for loading data and translating it to an intermediate representation. This creates a large step that leads to worse UX, as the user waits on the same step for longer, and makes debugging harder.

### Acceptance Criteria
- Loading is its own separate step.
- Translating market data is its own separate step.
