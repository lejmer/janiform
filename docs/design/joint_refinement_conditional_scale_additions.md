# Joint Refinement Conditional Scale Additions

## Summary
The revised joint-refinement objective fixed an important problem by adding explicit:

- return-scale scoring
- activity-level scoring
- clearer separation between shape, scale, and coupling

That helped remove the old "shape only" failure mode, but it still leaves a major blind spot:

- activity can be calibrated well
- RV versus activity co-movement can be calibrated well
- global return-scale metrics can improve
- yet returns can still be far too large conditional on time of day and conditional on business-time scale

In other words, the objective still does not directly ask whether the model produces the right return magnitude **given the local regime it is in**.

## Why The Current Objective Is Still Missing Something
The current return-scale bucket is mainly a whole-window magnitude bucket. That is useful, but it can still average over the exact failure mode we now care about:

- the model may put too much return amplitude into many or most intraday regimes
- the model may produce too-large returns for a given amount of activity
- those errors can survive even when hourly activity and RV/activity coupling look good

This is exactly the pattern we want to block next.

## Addition 1: Intraday Return Level Fit
Add a bucket that compares observed versus simulated return magnitude by hour of day.

Suggested target statistics:

- `15m mean absolute return by UTC hour`
- optionally `15m realized variance by UTC hour`

Why this is needed:

- the current objective scores hourly activity, but not hourly return magnitude
- if the model gets the activity profile right but still overstates return amplitude throughout the day, the objective should penalize that directly
- this bucket targets the visible failure mode where the generated hourly return profile is systematically too high

What it aims to solve:

- wrong intraday return-amplitude profile even when activity profile is good
- solutions that match the timing of activity but not the size of returns

Interpretation:

- this is an intraday **conditional scale** bucket
- it should answer: "does the model produce the right return level at each hour, not just overall?"

## Addition 2: Return Scale Versus Business-Time Fit
Add a bucket that compares observed versus simulated return magnitude conditional on business-time scale.

Suggested target statistics by business-time bucket:

- mean absolute return
- realized variance
- high-quantile absolute return such as `q90`

Why this is needed:

- the model can currently generate returns that are too large for the amount of activity present in a block
- RV/activity coupling can still look good when both series move together but the return amplitude is wrong relative to the activity regime
- whole-window scale metrics are too coarse to fully detect this

What it aims to solve:

- too-large block returns for a given activity level
- bad absolute-return versus business-time relationship even when aggregate activity is realistic
- solutions that preserve co-movement while mispricing the marginal scale of returns

Interpretation:

- this is the main structural **activity-conditioned return-scale** bucket
- it should answer: "given this business-time level, are returns the right size?"

## Expected Effect
Together, these additions should make the objective reject candidates that currently slip through because they have:

- good hourly activity shape
- acceptable activity level
- good RV/activity coupling
- but excessive return amplitude across hours or across business-time regimes

The intended behavior after adding them is:

- hourly activity remains well calibrated
- return magnitude by hour gets materially closer to observed
- return magnitude conditional on business-time scale gets materially closer to observed
- RV/activity remains a coupling diagnostic instead of acting like a hidden proxy for correct return scale

## Practical Guidance
These additions should be treated as **new levers**, not just a reason to retune weights on the existing buckets.

Recommended order:

1. add the two new buckets
2. verify each bucket independently on the EUR/USD workflow
3. only then rebalance weights

If only one addition is implemented first, the higher-value first step is:

- return scale versus business-time fit

That bucket addresses the more structural problem and should help the hourly return profile as a downstream consequence.
