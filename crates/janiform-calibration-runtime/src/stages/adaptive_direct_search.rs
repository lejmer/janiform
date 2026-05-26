use janiform_foundations::numeric::clamp_point;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct AdaptiveDirectSearchSettings {
    pub absolute_tolerance: f64,
    pub relative_tolerance: f64,
    pub initial_mesh_divisor: usize,
    pub failed_poll_mesh_refinement_divisor: usize,
    pub evaluation_budget: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AdaptiveDirectSearchResult {
    pub best_point: Vec<f64>,
    pub best_objective: f64,
    pub evaluation_count: usize,
    pub stop_reason: &'static str,
}

pub(crate) fn optimize_bounded_adaptive_direct_search<F>(
    initial_point: &[f64],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
    settings: AdaptiveDirectSearchSettings,
    evaluate: &mut F,
) -> Option<AdaptiveDirectSearchResult>
where
    F: FnMut(&[f64]) -> Option<f64>,
{
    if initial_point.is_empty()
        || initial_point.len() != lower_bounds.len()
        || initial_point.len() != upper_bounds.len()
        || settings.initial_mesh_divisor == 0
        || settings.failed_poll_mesh_refinement_divisor < 2
        || settings.evaluation_budget == 0
    {
        return None;
    }

    let mut current_point = clamp_point(initial_point, lower_bounds, upper_bounds);
    let mut current_objective = evaluate(current_point.as_slice())?;

    // Keep an owned best point because the current poll center may move after
    // evaluating a better candidate.
    let mut best_point = current_point.clone();
    let mut best_objective = current_objective;
    let mut evaluation_count = 1usize;

    let mut mesh_radii = initial_mesh_radii(
        lower_bounds,
        upper_bounds,
        settings.initial_mesh_divisor,
        settings.absolute_tolerance,
    );

    let poll_directions = adaptive_direct_search_poll_directions(initial_point.len());

    while evaluation_count < settings.evaluation_budget {
        if mesh_is_at_tolerance(
            mesh_radii.as_slice(),
            current_point.as_slice(),
            settings.absolute_tolerance,
            settings.relative_tolerance,
        ) {
            return Some(AdaptiveDirectSearchResult {
                best_point,
                best_objective,
                evaluation_count,
                stop_reason: "tolerance",
            });
        }

        let poll_start_objective = current_objective;

        let mut best_poll_point: Option<Vec<f64>> = None;
        let mut best_poll_objective = current_objective;

        for direction in &poll_directions {
            if evaluation_count >= settings.evaluation_budget {
                return Some(AdaptiveDirectSearchResult {
                    best_point,
                    best_objective,
                    evaluation_count,
                    stop_reason: "budget",
                });
            }

            let candidate_point = mesh_poll_candidate(
                current_point.as_slice(),
                direction.as_slice(),
                mesh_radii.as_slice(),
                lower_bounds,
                upper_bounds,
            );
            if candidate_point == current_point {
                continue;
            }

            evaluation_count += 1;

            let Some(candidate_objective) = evaluate(candidate_point.as_slice()) else {
                continue;
            };

            if candidate_objective < best_objective {
                // Preserve the best feasible probe independently from the
                // current poll winner, which is accepted only after the full poll.
                best_point = candidate_point.clone();
                best_objective = candidate_objective;
            }

            if objective_improves(
                candidate_objective,
                best_poll_objective,
                settings.absolute_tolerance,
                settings.relative_tolerance,
            ) {
                best_poll_point = Some(candidate_point);
                best_poll_objective = candidate_objective;
            }
        }

        if let Some(poll_point) = best_poll_point {
            current_point = poll_point;
            current_objective = best_poll_objective;
            continue;
        }

        if !refine_mesh_radii(
            mesh_radii.as_mut_slice(),
            settings.failed_poll_mesh_refinement_divisor,
            settings.absolute_tolerance,
        ) {
            return Some(AdaptiveDirectSearchResult {
                best_point,
                best_objective,
                evaluation_count,
                stop_reason: "invalid_step",
            });
        }

        if !objective_improves(
            current_objective,
            poll_start_objective,
            settings.absolute_tolerance,
            settings.relative_tolerance,
        ) && mesh_is_at_tolerance(
            mesh_radii.as_slice(),
            current_point.as_slice(),
            settings.absolute_tolerance,
            settings.relative_tolerance,
        ) {
            return Some(AdaptiveDirectSearchResult {
                best_point,
                best_objective,
                evaluation_count,
                stop_reason: "tolerance",
            });
        }
    }

    Some(AdaptiveDirectSearchResult {
        best_point,
        best_objective,
        evaluation_count,
        stop_reason: "budget",
    })
}

pub(crate) fn initial_mesh_radii(
    lower_bounds: &[f64],
    upper_bounds: &[f64],
    initial_mesh_divisor: usize,
    absolute_tolerance: f64,
) -> Vec<f64> {
    lower_bounds
        .iter()
        .zip(upper_bounds.iter())
        .map(|(lower_bound, upper_bound)| {
            let width = upper_bound - lower_bound;
            if width.is_finite() && width > 0.0 {
                (width / initial_mesh_divisor as f64).max(absolute_tolerance)
            } else {
                absolute_tolerance
            }
        })
        .collect()
}

pub(crate) fn adaptive_direct_search_poll_directions(dimension: usize) -> Vec<Vec<f64>> {
    let mut directions = Vec::with_capacity(dimension.saturating_mul(2).saturating_add(2));

    for coordinate_index in 0..dimension {
        let mut positive_direction = vec![0.0; dimension];
        positive_direction[coordinate_index] = 1.0;

        directions.push(positive_direction);

        let mut negative_direction = vec![0.0; dimension];
        negative_direction[coordinate_index] = -1.0;

        directions.push(negative_direction);
    }

    if dimension > 1 {
        let normalized_component = (dimension as f64).sqrt().recip();

        directions.push(vec![normalized_component; dimension]);
        directions.push(vec![-normalized_component; dimension]);
    }

    directions
}

pub(crate) fn mesh_poll_candidate(
    current_point: &[f64],
    direction: &[f64],
    mesh_radii: &[f64],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) -> Vec<f64> {
    let candidate_point = current_point
        .iter()
        .zip(direction.iter().zip(mesh_radii.iter()))
        .map(|(coordinate, (direction_component, mesh_radius))| {
            coordinate + direction_component * mesh_radius
        })
        .collect::<Vec<_>>();

    clamp_point(candidate_point.as_slice(), lower_bounds, upper_bounds)
}

pub(crate) fn objective_improves(
    candidate_objective: f64,
    current_objective: f64,
    absolute_tolerance: f64,
    relative_tolerance: f64,
) -> bool {
    let objective_tolerance =
        absolute_tolerance.max(relative_tolerance * current_objective.abs().max(1.0));

    candidate_objective + objective_tolerance < current_objective
}

pub(crate) fn refine_mesh_radii(
    mesh_radii: &mut [f64],
    failed_poll_mesh_refinement_divisor: usize,
    absolute_tolerance: f64,
) -> bool {
    let divisor = failed_poll_mesh_refinement_divisor as f64;

    let mut any_radius_changed = false;

    for mesh_radius in mesh_radii.iter_mut() {
        let refined_radius = (*mesh_radius / divisor).max(absolute_tolerance);
        if refined_radius < *mesh_radius {
            *mesh_radius = refined_radius;
            any_radius_changed = true;
        }
    }

    any_radius_changed
}

pub(crate) fn mesh_is_at_tolerance(
    mesh_radii: &[f64],
    current_point: &[f64],
    absolute_tolerance: f64,
    relative_tolerance: f64,
) -> bool {
    mesh_radii
        .iter()
        .zip(current_point.iter())
        .all(|(mesh_radius, coordinate)| {
            *mesh_radius <= absolute_tolerance.max(relative_tolerance * coordinate.abs().max(1.0))
        })
}

#[cfg(test)]
mod tests {
    use super::{
        AdaptiveDirectSearchSettings, adaptive_direct_search_poll_directions,
        optimize_bounded_adaptive_direct_search,
    };

    #[test]
    fn adaptive_direct_search_improves_bounded_quadratic() {
        let settings = AdaptiveDirectSearchSettings {
            absolute_tolerance: 1.0e-8,
            relative_tolerance: 1.0e-8,
            initial_mesh_divisor: 3,
            failed_poll_mesh_refinement_divisor: 2,
            evaluation_budget: 80,
        };

        let mut evaluate = |point: &[f64]| -> Option<f64> {
            Some((point[0] - 0.25).powi(2) + (point[1] + 0.5).powi(2))
        };

        let result = optimize_bounded_adaptive_direct_search(
            &[0.0, 0.0],
            &[-1.0, -1.0],
            &[1.0, 1.0],
            settings,
            &mut evaluate,
        )
        .expect("valid direct-search problem should produce a result");

        assert!(result.best_objective < 1.0e-4);
        assert!(result.best_point[0] > 0.2 && result.best_point[0] < 0.3);
        assert!(result.best_point[1] > -0.55 && result.best_point[1] < -0.45);
    }

    #[test]
    fn adaptive_direct_search_skips_invalid_poll_points() {
        let settings = AdaptiveDirectSearchSettings {
            absolute_tolerance: 1.0e-8,
            relative_tolerance: 1.0e-8,
            initial_mesh_divisor: 2,
            failed_poll_mesh_refinement_divisor: 2,
            evaluation_budget: 40,
        };

        let mut evaluate = |point: &[f64]| -> Option<f64> {
            (point[0] <= 0.75).then_some((point[0] - 0.5).powi(2))
        };

        let result = optimize_bounded_adaptive_direct_search(
            &[0.0],
            &[0.0],
            &[1.0],
            settings,
            &mut evaluate,
        )
        .expect("valid initial point should produce a result");

        assert!(result.best_objective < 1.0e-4);
        assert!(result.best_point[0] > 0.45 && result.best_point[0] < 0.55);
    }

    #[test]
    fn adaptive_direct_search_poll_directions_include_coupled_moves() {
        let directions = adaptive_direct_search_poll_directions(3);

        assert_eq!(directions.len(), 8);
        assert_eq!(directions[0], vec![1.0, 0.0, 0.0]);
        assert_eq!(directions[1], vec![-1.0, 0.0, 0.0]);
        assert!(directions[6].iter().all(|component| *component > 0.0));
        assert!(directions[7].iter().all(|component| *component < 0.0));
    }
}
