use janiform_foundations::numeric::clamp_point;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PowellOptimizerSettings {
    pub absolute_tolerance: f64,
    pub relative_tolerance: f64,
    pub min_step_norm: f64,
    pub max_cycle_count: usize,
    pub line_search_evaluation_cap: usize,
    pub evaluation_budget: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PowellOptimizerResult {
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
