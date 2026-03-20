# Joint Refinement Objective Revision

## Summary
The current `joint_refinement` objective is materially better than it used to be, but it still allows a bad amplitude tradeoff:

- hourly activity shape can be right while activity level is far too low
- volatility and squared-return clustering can look good while block returns are far too large
- rolling realized variance versus rolling activity correlation can be very strong even when both the activity level and return scale are wrong

The next revision of the objective should therefore make **absolute level calibration** matter much more, not just shape and co-movement.

This is not a request to "just change weights". The main change is to add stronger **level-sensitive terms** and to narrow the scope of the existing terms so they cannot reward the wrong amplitudes.

## Current Failure Mode
Observed behavior after the current joint-refinement implementation:

- hourly activity profile shape is good, but generated activity can be far below observed activity
- rolling RV versus rolling activity correlation is good, but that metric is too insensitive to absolute scale
- block return distribution is still badly wrong
- generated block returns can be much too large even when the shape-oriented diagnostics improve

In practical terms, the optimizer can currently get rewarded for:

- the right activity shape with the wrong activity level
- the right volatility clustering pattern with the wrong return scale
- the right co-movement between RV and activity with the wrong marginal distributions

## Objective Design Change
The joint objective should be revised so that:

1. activity **level** matters directly
2. return **scale** matters directly
3. RV versus activity remains important, but only as a **co-movement** diagnostic
4. profile-shape terms remain, but they no longer carry the burden of level calibration

## Proposed Bucket Structure

### 1. Aggregated Likelihood Fit
Keep the current aggregated conditional likelihood term, but do not rely on it alone to enforce realistic return scale.

Purpose:

- preserve the conditional continuous-layer fit on aggregated blocks
- keep the optimizer anchored to a statistically coherent return law

Suggested role:

- continue using `5m` and `15m`
- keep as a major term, but not the only return-distribution term

### 2. Aggregated Return Shape Fit
Retain a shape-focused simulated-versus-observed return distribution comparison, but treat it explicitly as a **shape** term rather than a general-purpose distribution term.

Purpose:

- compare profile shape, asymmetry, and tail structure
- preserve the gains already achieved in multi-horizon path behavior

Important change:

- this bucket should not be expected to carry return-scale calibration by itself

### 3. Aggregated Return Scale Fit
Add a new model-level return-scale bucket computed from simulated aggregated intervals.

Purpose:

- directly penalize the current failure mode where block returns are much too large

Suggested components:

- `15m mean absolute return`
- `15m realized variance`
- `15m high-quantile absolute return`, for example `q90` or `q99`
- optionally the same metrics on `5m`, but `15m` should carry more weight

Suggested construction:

- compare simulated versus observed values using relative error or log-ratio distance
- average across simulation seeds in fixed order
- aggregate into one normalized bucket

Interpretation:

- this bucket is about **magnitude**
- the existing shape bucket is about **form**

### 4. Activity Shape Fit
Keep an activity-profile shape term, but separate it from level.

Purpose:

- preserve the fact that the model now gets the shape of the hourly activity profile much more right

Suggested components:

- normalized hourly profile distance
- maybe z-scored hourly path distance over the training window

Important change:

- this bucket should reward shape only
- it should no longer be the only activity-facing term

### 5. Activity Level Fit
Add a new explicit activity-level bucket.

Purpose:

- directly penalize cases where generated hourly activity is only a fraction of observed activity even when the shape is right

Suggested components:

- total open-session tick count ratio
- mean hourly tick count ratio
- peak hourly tick count ratio
- optionally RMSE on the raw hourly count path without z-scoring

Suggested construction:

- use observed versus simulated hourly counts over the full training window
- compute level-sensitive distances only
- normalize relative to the staged baseline

Interpretation:

- this is the main term that should stop the optimizer from accepting "perfect shape at one-third the activity"

### 6. RV Versus Activity Coupling Fit
Keep this term, but narrow its role.

Purpose:

- reward realistic volatility clustering driven by activity
- preserve the important co-movement structure in the original data

Important change:

- this bucket should be treated as a **coupling** term, not a hidden proxy for level calibration

Suggested components:

- rolling RV/activity correlation difference
- standardized rolling-path distance between RV and activity

Explicit non-goal:

- do not expect this bucket to enforce the right activity level or return scale

### 7. Price Span Sanity Fit
Keep the price-span sanity term.

Purpose:

- prevent clearly pathological span inflation

Role:

- remain a guardrail and sanity bucket
- it is useful, but it should not carry the core calibration burden

## Proposed Reweighting Direction
Exact weights should be tuned after the new buckets are added, but the direction should be:

- less reliance on a single broad "shape" concept
- less reliance on RV/activity as a proxy for overall realism
- more weight on explicit activity level and return scale terms

One reasonable starting structure would be:

- aggregated likelihood fit: `0.20` to `0.25`
- aggregated return shape fit: `0.10` to `0.15`
- aggregated return scale fit: `0.20` to `0.25`
- activity shape fit: `0.10` to `0.15`
- activity level fit: `0.15` to `0.20`
- RV/activity coupling fit: `0.10` to `0.15`
- price span sanity fit: `0.05` to `0.10`

This should be treated as an initialization point, not a final policy.

## Design Principles
The revised objective should follow these principles:

1. Separate **shape**, **scale**, and **coupling**
2. Do not let one scalar correlation metric stand in for level calibration
3. Do not let a shape bucket hide large magnitude errors
4. Keep all simulated terms deterministic under fixed seeds and fixed aggregation order
5. Continue normalizing buckets against the staged baseline so improvement remains interpretable
6. Keep all implementations DRY (look for existing helpers before writing new ones, never duplicate helpers or code patterns unnecessarily)
7. A key objective is to keep the calibration process computationally efficient, so do not take shortcuts that slow down calibration more than we have to by introducing new pieces

## Expected Effect
If the revised objective works as intended, the optimizer should stop preferring solutions with:

- too little activity but the right activity profile shape
- too-large conditional moves compensating for under-activity
- good RV/activity co-movement but bad marginal return distributions

The intended new behavior is:

- hourly activity profile shape stays good
- hourly activity level gets materially closer to observed
- block return scale comes down toward observed
- RV/activity coupling remains strong, but no longer dominates the ranking in the wrong way

## Immediate Implementation Guidance
When implementing this revision:

- add the new buckets first
- verify each bucket independently on the EUR/USD playground workflow
- only then rebalance weights
- keep the existing deterministic caching and fixed-seed evaluation rules

The core idea is straightforward:

- make **activity level** matter
- make **return scale** matter
- let **coupling** remain important, but only for coupling
