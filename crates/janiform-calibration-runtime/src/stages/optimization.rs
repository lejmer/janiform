#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PowellOptimizerSettings {
    pub absolute_tolerance: f64,
    pub relative_tolerance: f64,
    pub min_step_norm: f64,
    pub max_cycle_count: usize,
    pub line_search_evaluation_cap: usize,
    pub evaluation_budget: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BoundedTrustRegionOptimizerSettings {
    pub absolute_tolerance: f64,
    pub relative_tolerance: f64,
    pub minimum_trust_region_radius: f64,
    pub initial_trust_region_radius_fraction: f64,
    pub interpolation_point_target: usize,
    pub evaluation_budget: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PowellOptimizerResult {
    pub best_point: Vec<f64>,
    pub best_objective: f64,
    pub evaluation_count: usize,
    pub stop_reason: &'static str,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BoundedTrustRegionOptimizerResult {
    pub best_point: Vec<f64>,
    pub best_objective: f64,
    pub evaluation_count: usize,
    pub stop_reason: &'static str,
}

#[derive(Debug, Clone)]
struct LineSearchOutcome {
    best_point: Vec<f64>,
    best_objective: Option<f64>,
    evaluation_count: usize,
}

#[derive(Debug, Clone)]
struct InterpolationSample {
    point: Vec<f64>,
    objective: f64,
}

pub(crate) fn optimize_bounded_powell<F>(
    initial_point: &[f64],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
    settings: PowellOptimizerSettings,
    evaluate: &mut F,
) -> Option<PowellOptimizerResult>
where
    F: FnMut(&[f64]) -> Option<f64>,
{
    if initial_point.is_empty()
        || initial_point.len() != lower_bounds.len()
        || initial_point.len() != upper_bounds.len()
    {
        return None;
    }

    let dimension = initial_point.len();

    let mut current_point = clamp_point(initial_point, lower_bounds, upper_bounds);
    let mut current_objective = evaluate(current_point.as_slice())?;

    let mut best_point = current_point.clone();
    let mut best_objective = current_objective;
    let mut evaluation_count = 1usize;

    let mut directions = (0..dimension)
        .map(|coordinate_index| {
            let mut direction = vec![0.0; dimension];
            direction[coordinate_index] = 1.0;
            direction
        })
        .collect::<Vec<_>>();

    for _ in 0..settings.max_cycle_count {
        let cycle_start_point = current_point.clone();
        let cycle_start_objective = current_objective;

        let mut largest_direction_improvement = 0.0;
        let mut replaced_direction_index = 0usize;

        for (direction_index, direction) in directions.iter().enumerate() {
            let line_search_outcome = minimize_along_direction(
                current_point.as_slice(),
                direction.as_slice(),
                lower_bounds,
                upper_bounds,
                settings,
                evaluate,
            );

            evaluation_count += line_search_outcome.evaluation_count;
            if evaluation_count >= settings.evaluation_budget {
                return Some(PowellOptimizerResult {
                    best_point,
                    best_objective,
                    evaluation_count,
                    stop_reason: "budget",
                });
            }

            if let Some(line_objective) = line_search_outcome.best_objective {
                let direction_improvement = current_objective - line_objective;
                if direction_improvement > largest_direction_improvement {
                    largest_direction_improvement = direction_improvement;
                    replaced_direction_index = direction_index;
                }

                current_point = line_search_outcome.best_point;
                current_objective = line_objective;
                if current_objective < best_objective {
                    best_point = current_point.clone();
                    best_objective = current_objective;
                }
            }
        }

        let cycle_displacement =
            subtract_points(current_point.as_slice(), cycle_start_point.as_slice());

        let maximum_coordinate_displacement = cycle_displacement
            .iter()
            .map(|coordinate| coordinate.abs())
            .fold(0.0, f64::max);

        let cycle_improvement = cycle_start_objective - current_objective;
        let objective_tolerance = settings
            .absolute_tolerance
            .max(settings.relative_tolerance * cycle_start_objective.abs().max(1.0));
        if cycle_improvement <= objective_tolerance
            || maximum_coordinate_displacement <= settings.absolute_tolerance
        {
            return Some(PowellOptimizerResult {
                best_point,
                best_objective,
                evaluation_count,
                stop_reason: "tolerance",
            });
        }

        if maximum_coordinate_displacement <= settings.min_step_norm {
            return Some(PowellOptimizerResult {
                best_point,
                best_objective,
                evaluation_count,
                stop_reason: "invalid_step",
            });
        }

        let composite_direction = cycle_displacement;

        let composite_line_search_outcome = minimize_along_direction(
            current_point.as_slice(),
            composite_direction.as_slice(),
            lower_bounds,
            upper_bounds,
            settings,
            evaluate,
        );

        evaluation_count += composite_line_search_outcome.evaluation_count;
        if evaluation_count >= settings.evaluation_budget {
            return Some(PowellOptimizerResult {
                best_point,
                best_objective,
                evaluation_count,
                stop_reason: "budget",
            });
        }

        if let Some(composite_objective) = composite_line_search_outcome.best_objective {
            if composite_objective < current_objective {
                current_point = composite_line_search_outcome.best_point;
                current_objective = composite_objective;
                if current_objective < best_objective {
                    best_point = current_point.clone();
                    best_objective = current_objective;
                }
            }

            let composite_improvement = cycle_start_objective - composite_objective;
            if composite_improvement > largest_direction_improvement {
                directions[replaced_direction_index] =
                    normalize_direction(composite_direction.as_slice(), settings.min_step_norm);
            }
        }
    }

    Some(PowellOptimizerResult {
        best_point,
        best_objective,
        evaluation_count,
        stop_reason: "budget",
    })
}

pub(crate) fn optimize_bounded_trust_region_quadratic<F>(
    initial_point: &[f64],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
    settings: BoundedTrustRegionOptimizerSettings,
    evaluate: &mut F,
) -> Option<BoundedTrustRegionOptimizerResult>
where
    F: FnMut(&[f64]) -> Option<f64>,
{
    if initial_point.is_empty()
        || initial_point.len() != lower_bounds.len()
        || initial_point.len() != upper_bounds.len()
    {
        return None;
    }

    let dimension = initial_point.len();
    let current_point = clamp_point(initial_point, lower_bounds, upper_bounds);
    let current_objective = evaluate(current_point.as_slice())?;
    let mut evaluation_count = 1usize;
    let mut best_point = current_point.clone();
    let mut best_objective = current_objective;
    let mut sample_set = vec![InterpolationSample {
        point: current_point.clone(),
        objective: current_objective,
    }];

    let mut trust_region_radius = initial_trust_region_radius(
        current_point.as_slice(),
        lower_bounds,
        upper_bounds,
        settings,
    );
    if trust_region_radius <= 0.0 {
        return Some(BoundedTrustRegionOptimizerResult {
            best_point,
            best_objective,
            evaluation_count,
            stop_reason: "degenerate_domain",
        });
    }

    let target_sample_count = settings
        .interpolation_point_target
        .max(dimension + 1)
        .min(settings.evaluation_budget.max(1));

    let initial_samples = build_initial_interpolation_samples(
        current_point.as_slice(),
        lower_bounds,
        upper_bounds,
        trust_region_radius,
        settings.minimum_trust_region_radius,
    );

    for sample_point in initial_samples {
        if evaluation_count >= settings.evaluation_budget {
            break;
        }
        if sample_set.iter().any(|sample| sample.point == sample_point) {
            continue;
        }

        if let Some(sample_objective) = evaluate(sample_point.as_slice()) {
            evaluation_count += 1;
            if sample_objective < best_objective {
                best_point = sample_point.clone();
                best_objective = sample_objective;
            }
            sample_set.push(InterpolationSample {
                point: sample_point,
                objective: sample_objective,
            });
        }
    }

    while evaluation_count < settings.evaluation_budget {
        sample_set.sort_by(|left, right| left.objective.total_cmp(&right.objective));
        if sample_set.len() > target_sample_count {
            sample_set.truncate(target_sample_count);
        }

        let Some(quadratic_model) =
            fit_diagonal_quadratic_model(sample_set.as_slice(), best_point.as_slice())
        else {
            return Some(BoundedTrustRegionOptimizerResult {
                best_point,
                best_objective,
                evaluation_count,
                stop_reason: "insufficient_model_support",
            });
        };

        let predicted_point = propose_trust_region_candidate(
            best_point.as_slice(),
            lower_bounds,
            upper_bounds,
            trust_region_radius,
            &quadratic_model,
        );

        if max_coordinate_distance(predicted_point.as_slice(), best_point.as_slice())
            <= settings.minimum_trust_region_radius
        {
            return Some(BoundedTrustRegionOptimizerResult {
                best_point,
                best_objective,
                evaluation_count,
                stop_reason: "trust_region_tolerance",
            });
        }

        let predicted_objective = evaluate_diagonal_quadratic_model(
            &quadratic_model,
            best_point.as_slice(),
            predicted_point.as_slice(),
        );
        let Some(actual_objective) = evaluate(predicted_point.as_slice()) else {
            trust_region_radius *= 0.5;
            if trust_region_radius <= settings.minimum_trust_region_radius {
                return Some(BoundedTrustRegionOptimizerResult {
                    best_point,
                    best_objective,
                    evaluation_count,
                    stop_reason: "invalid_step",
                });
            }
            continue;
        };
        evaluation_count += 1;

        let actual_improvement = best_objective - actual_objective;
        let predicted_improvement = best_objective - predicted_objective;
        let acceptance_ratio = if predicted_improvement <= 0.0 {
            0.0
        } else {
            actual_improvement / predicted_improvement
        };

        sample_set.push(InterpolationSample {
            point: predicted_point.clone(),
            objective: actual_objective,
        });

        if actual_objective < best_objective {
            let objective_tolerance = settings
                .absolute_tolerance
                .max(settings.relative_tolerance * best_objective.abs().max(1.0));
            best_point = predicted_point;
            if actual_improvement <= objective_tolerance
                && trust_region_radius <= settings.minimum_trust_region_radius * 2.0
            {
                best_objective = actual_objective;
                return Some(BoundedTrustRegionOptimizerResult {
                    best_point,
                    best_objective,
                    evaluation_count,
                    stop_reason: "tolerance",
                });
            }
            best_objective = actual_objective;
        }

        trust_region_radius = update_trust_region_radius(
            trust_region_radius,
            acceptance_ratio,
            best_point.as_slice(),
            lower_bounds,
            upper_bounds,
            settings.minimum_trust_region_radius,
        );

        if trust_region_radius <= settings.minimum_trust_region_radius {
            return Some(BoundedTrustRegionOptimizerResult {
                best_point,
                best_objective,
                evaluation_count,
                stop_reason: "trust_region_tolerance",
            });
        }
    }

    Some(BoundedTrustRegionOptimizerResult {
        best_point,
        best_objective,
        evaluation_count,
        stop_reason: "budget",
    })
}

fn minimize_along_direction<F>(
    current_point: &[f64],
    direction: &[f64],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
    settings: PowellOptimizerSettings,
    evaluate: &mut F,
) -> LineSearchOutcome
where
    F: FnMut(&[f64]) -> Option<f64>,
{
    let Some((lower_step, upper_step)) =
        bounded_step_interval(current_point, direction, lower_bounds, upper_bounds)
    else {
        return LineSearchOutcome {
            best_point: current_point.to_vec(),
            best_objective: evaluate(current_point),
            evaluation_count: 0,
        };
    };

    if (upper_step - lower_step).abs() <= settings.absolute_tolerance {
        return LineSearchOutcome {
            best_point: current_point.to_vec(),
            best_objective: evaluate(current_point),
            evaluation_count: 0,
        };
    }

    let mut best_point = current_point.to_vec();
    let mut best_objective = evaluate(current_point);
    let mut evaluation_count = 0usize;

    let golden_ratio = (5.0_f64.sqrt() - 1.0) * 0.5;

    let mut left = lower_step;
    let mut right = upper_step;
    let mut left_inner = right - golden_ratio * (right - left);
    let mut right_inner = left + golden_ratio * (right - left);

    let mut left_inner_evaluation = evaluate_line_search_step(
        current_point,
        direction,
        left_inner,
        lower_bounds,
        upper_bounds,
        evaluate,
    );

    let mut right_inner_evaluation = evaluate_line_search_step(
        current_point,
        direction,
        right_inner,
        lower_bounds,
        upper_bounds,
        evaluate,
    );

    evaluation_count += 2;

    update_best_line_search_evaluation(
        left_inner_evaluation.as_ref(),
        &mut best_point,
        &mut best_objective,
    );
    update_best_line_search_evaluation(
        right_inner_evaluation.as_ref(),
        &mut best_point,
        &mut best_objective,
    );

    while evaluation_count < settings.line_search_evaluation_cap {
        let left_objective = left_inner_evaluation
            .as_ref()
            .map(|(_, objective)| *objective)
            .unwrap_or(f64::INFINITY);
        let right_objective = right_inner_evaluation
            .as_ref()
            .map(|(_, objective)| *objective)
            .unwrap_or(f64::INFINITY);

        if left_objective <= right_objective {
            right = right_inner;
            right_inner = left_inner;
            right_inner_evaluation = left_inner_evaluation.clone();

            left_inner = right - golden_ratio * (right - left);
            left_inner_evaluation = evaluate_line_search_step(
                current_point,
                direction,
                left_inner,
                lower_bounds,
                upper_bounds,
                evaluate,
            );

            evaluation_count += 1;

            update_best_line_search_evaluation(
                left_inner_evaluation.as_ref(),
                &mut best_point,
                &mut best_objective,
            );
        } else {
            left = left_inner;
            left_inner = right_inner;
            left_inner_evaluation = right_inner_evaluation.clone();

            right_inner = left + golden_ratio * (right - left);
            right_inner_evaluation = evaluate_line_search_step(
                current_point,
                direction,
                right_inner,
                lower_bounds,
                upper_bounds,
                evaluate,
            );

            evaluation_count += 1;

            update_best_line_search_evaluation(
                right_inner_evaluation.as_ref(),
                &mut best_point,
                &mut best_objective,
            );
        }

        if (right - left).abs() <= settings.absolute_tolerance {
            break;
        }
    }

    LineSearchOutcome {
        best_point,
        best_objective,
        evaluation_count,
    }
}

#[derive(Debug, Clone)]
struct DiagonalQuadraticModel {
    gradient: Vec<f64>,
    curvature: Vec<f64>,
}

fn build_initial_interpolation_samples(
    center_point: &[f64],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
    trust_region_radius: f64,
    minimum_trust_region_radius: f64,
) -> Vec<Vec<f64>> {
    let mut samples = Vec::with_capacity(center_point.len() * 2);

    for coordinate_index in 0..center_point.len() {
        let mut positive_point = center_point.to_vec();
        let positive_step = (center_point[coordinate_index] + trust_region_radius)
            .min(upper_bounds[coordinate_index])
            - center_point[coordinate_index];
        if positive_step.abs() >= minimum_trust_region_radius {
            positive_point[coordinate_index] += positive_step;
            samples.push(positive_point);
        }

        let mut negative_point = center_point.to_vec();
        let negative_step = center_point[coordinate_index]
            - (center_point[coordinate_index] - trust_region_radius)
                .max(lower_bounds[coordinate_index]);
        if negative_step.abs() >= minimum_trust_region_radius {
            negative_point[coordinate_index] -= negative_step;
            samples.push(negative_point);
        }
    }

    samples
}

fn fit_diagonal_quadratic_model(
    samples: &[InterpolationSample],
    center_point: &[f64],
) -> Option<DiagonalQuadraticModel> {
    if samples.is_empty() {
        return None;
    }

    let center_objective = samples
        .iter()
        .find(|sample| sample.point == center_point)
        .map(|sample| sample.objective)?;
    let dimension = center_point.len();
    let mut gradient = vec![0.0; dimension];
    let mut curvature = vec![0.0; dimension];

    for coordinate_index in 0..dimension {
        let mut positive_sample: Option<(&InterpolationSample, f64)> = None;
        let mut negative_sample: Option<(&InterpolationSample, f64)> = None;

        for sample in samples {
            let displacement = sample.point[coordinate_index] - center_point[coordinate_index];
            if displacement == 0.0
                || point_differs_outside_coordinate(
                    sample.point.as_slice(),
                    center_point,
                    coordinate_index,
                )
            {
                continue;
            }

            if displacement > 0.0
                && positive_sample
                    .as_ref()
                    .is_none_or(|(_, existing_displacement)| {
                        displacement.abs() < existing_displacement.abs()
                    })
            {
                positive_sample = Some((sample, displacement));
            }

            if displacement < 0.0
                && negative_sample
                    .as_ref()
                    .is_none_or(|(_, existing_displacement)| {
                        displacement.abs() < existing_displacement.abs()
                    })
            {
                negative_sample = Some((sample, displacement));
            }
        }

        match (positive_sample, negative_sample) {
            (
                Some((positive_sample, positive_displacement)),
                Some((negative_sample, negative_displacement)),
            ) => {
                let positive_step = positive_displacement.abs();
                let negative_step = negative_displacement.abs();
                let symmetric_step = positive_step.min(negative_step);
                if symmetric_step == 0.0 {
                    return None;
                }
                gradient[coordinate_index] = (positive_sample.objective
                    - negative_sample.objective)
                    / (positive_step + negative_step);
                curvature[coordinate_index] = ((positive_sample.objective - center_objective)
                    / (positive_step * positive_step)
                    + (negative_sample.objective - center_objective)
                        / (negative_step * negative_step))
                    * 0.5;
            }
            (Some((positive_sample, positive_displacement)), None) => {
                let step = positive_displacement.abs();
                if step == 0.0 {
                    return None;
                }
                gradient[coordinate_index] = (positive_sample.objective - center_objective) / step;
                curvature[coordinate_index] = ((positive_sample.objective - center_objective)
                    - gradient[coordinate_index] * positive_displacement)
                    / (positive_displacement * positive_displacement);
            }
            (None, Some((negative_sample, negative_displacement))) => {
                let step = negative_displacement.abs();
                if step == 0.0 {
                    return None;
                }
                gradient[coordinate_index] = (center_objective - negative_sample.objective) / step;
                curvature[coordinate_index] = ((negative_sample.objective - center_objective)
                    - gradient[coordinate_index] * negative_displacement)
                    / (negative_displacement * negative_displacement);
            }
            (None, None) => {
                return None;
            }
        }
    }

    Some(DiagonalQuadraticModel {
        gradient,
        curvature,
    })
}

fn propose_trust_region_candidate(
    center_point: &[f64],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
    trust_region_radius: f64,
    model: &DiagonalQuadraticModel,
) -> Vec<f64> {
    let mut candidate = center_point.to_vec();

    for coordinate_index in 0..center_point.len() {
        let unconstrained_step = if model.curvature[coordinate_index].is_finite()
            && model.curvature[coordinate_index] > 0.0
        {
            -model.gradient[coordinate_index] / (2.0 * model.curvature[coordinate_index])
        } else {
            -model.gradient[coordinate_index].signum() * trust_region_radius
        };
        let bounded_step = unconstrained_step.clamp(-trust_region_radius, trust_region_radius);
        candidate[coordinate_index] = (center_point[coordinate_index] + bounded_step).clamp(
            lower_bounds[coordinate_index],
            upper_bounds[coordinate_index],
        );
    }

    candidate
}

fn evaluate_diagonal_quadratic_model(
    model: &DiagonalQuadraticModel,
    center_point: &[f64],
    candidate_point: &[f64],
) -> f64 {
    let mut value = 0.0;
    for coordinate_index in 0..center_point.len() {
        let displacement = candidate_point[coordinate_index] - center_point[coordinate_index];
        value += model.gradient[coordinate_index] * displacement
            + model.curvature[coordinate_index] * displacement * displacement;
    }
    value
}

fn update_trust_region_radius(
    current_radius: f64,
    acceptance_ratio: f64,
    best_point: &[f64],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
    minimum_trust_region_radius: f64,
) -> f64 {
    let expanded_radius =
        bounded_radius_from_point(best_point, lower_bounds, upper_bounds, current_radius * 2.0);
    if acceptance_ratio >= 0.75 {
        return expanded_radius.max(minimum_trust_region_radius);
    }
    if acceptance_ratio >= 0.1 {
        return current_radius.max(minimum_trust_region_radius);
    }
    (current_radius * 0.5).max(minimum_trust_region_radius)
}

fn initial_trust_region_radius(
    center_point: &[f64],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
    settings: BoundedTrustRegionOptimizerSettings,
) -> f64 {
    let widest_coordinate_span = lower_bounds
        .iter()
        .zip(upper_bounds)
        .map(|(lower_bound, upper_bound)| upper_bound - lower_bound)
        .fold(0.0, f64::max);
    let requested_radius =
        widest_coordinate_span * settings.initial_trust_region_radius_fraction.max(0.0);

    bounded_radius_from_point(
        center_point,
        lower_bounds,
        upper_bounds,
        requested_radius.max(settings.minimum_trust_region_radius),
    )
}

fn bounded_radius_from_point(
    center_point: &[f64],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
    requested_radius: f64,
) -> f64 {
    let admissible_radius = center_point
        .iter()
        .zip(lower_bounds)
        .zip(upper_bounds)
        .map(|((coordinate, lower_bound), upper_bound)| {
            (coordinate - lower_bound).min(upper_bound - coordinate)
        })
        .fold(requested_radius, f64::min);

    admissible_radius.max(0.0)
}

fn point_differs_outside_coordinate(
    sample_point: &[f64],
    center_point: &[f64],
    excluded_coordinate_index: usize,
) -> bool {
    sample_point.iter().zip(center_point).enumerate().any(
        |(coordinate_index, (sample_coordinate, center_coordinate))| {
            coordinate_index != excluded_coordinate_index
                && sample_coordinate.to_bits() != center_coordinate.to_bits()
        },
    )
}

fn max_coordinate_distance(left_point: &[f64], right_point: &[f64]) -> f64 {
    left_point
        .iter()
        .zip(right_point)
        .map(|(left_coordinate, right_coordinate)| (left_coordinate - right_coordinate).abs())
        .fold(0.0, f64::max)
}

fn evaluate_line_search_step<F>(
    current_point: &[f64],
    direction: &[f64],
    step: f64,
    lower_bounds: &[f64],
    upper_bounds: &[f64],
    evaluate: &mut F,
) -> Option<(Vec<f64>, f64)>
where
    F: FnMut(&[f64]) -> Option<f64>,
{
    let candidate_point = clamp_point(
        add_points(current_point, scale_point(direction, step).as_slice()).as_slice(),
        lower_bounds,
        upper_bounds,
    );

    let candidate_objective = evaluate(candidate_point.as_slice())?;

    Some((candidate_point, candidate_objective))
}

fn update_best_line_search_evaluation(
    evaluated_step: Option<&(Vec<f64>, f64)>,
    best_point: &mut Vec<f64>,
    best_objective: &mut Option<f64>,
) {
    let Some((candidate_point, candidate_objective)) = evaluated_step else {
        return;
    };

    let should_replace = best_objective
        .map(|current_best| *candidate_objective < current_best)
        .unwrap_or(true);
    if should_replace {
        *best_point = candidate_point.clone();
        *best_objective = Some(*candidate_objective);
    }
}

fn bounded_step_interval(
    current_point: &[f64],
    direction: &[f64],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) -> Option<(f64, f64)> {
    let mut lower_step = f64::NEG_INFINITY;
    let mut upper_step = f64::INFINITY;

    for coordinate_index in 0..current_point.len() {
        let direction_component = direction[coordinate_index];
        if direction_component.abs() <= f64::EPSILON {
            continue;
        }

        let step_to_lower = (lower_bounds[coordinate_index] - current_point[coordinate_index])
            / direction_component;
        let step_to_upper = (upper_bounds[coordinate_index] - current_point[coordinate_index])
            / direction_component;

        let coordinate_lower_step = step_to_lower.min(step_to_upper);
        let coordinate_upper_step = step_to_lower.max(step_to_upper);

        lower_step = lower_step.max(coordinate_lower_step);
        upper_step = upper_step.min(coordinate_upper_step);
    }

    (lower_step.is_finite() && upper_step.is_finite() && lower_step <= upper_step)
        .then_some((lower_step, upper_step))
}

fn clamp_point(point: &[f64], lower_bounds: &[f64], upper_bounds: &[f64]) -> Vec<f64> {
    point
        .iter()
        .zip(lower_bounds.iter().zip(upper_bounds.iter()))
        .map(|(coordinate, (lower_bound, upper_bound))| {
            coordinate.clamp(*lower_bound, *upper_bound)
        })
        .collect()
}

fn add_points(left: &[f64], right: &[f64]) -> Vec<f64> {
    left.iter()
        .zip(right.iter())
        .map(|(left_coordinate, right_coordinate)| left_coordinate + right_coordinate)
        .collect()
}

fn subtract_points(left: &[f64], right: &[f64]) -> Vec<f64> {
    left.iter()
        .zip(right.iter())
        .map(|(left_coordinate, right_coordinate)| left_coordinate - right_coordinate)
        .collect()
}

fn scale_point(point: &[f64], scalar: f64) -> Vec<f64> {
    point.iter().map(|coordinate| coordinate * scalar).collect()
}

fn normalize_direction(direction: &[f64], floor: f64) -> Vec<f64> {
    let norm = direction
        .iter()
        .map(|coordinate| coordinate * coordinate)
        .sum::<f64>()
        .sqrt();
    if !norm.is_finite() || norm <= floor {
        let mut basis = vec![0.0; direction.len()];
        if let Some(first_coordinate) = basis.first_mut() {
            *first_coordinate = 1.0;
        }
        return basis;
    }

    scale_point(direction, 1.0 / norm)
}
