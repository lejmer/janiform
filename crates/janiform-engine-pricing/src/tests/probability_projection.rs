use crate::probability_projection::probability_mass_from_sampled_cdf_values;

fn assert_probability_mass_close(observed: &[f64], expected: &[f64]) {
    assert_eq!(observed.len(), expected.len());

    for (observed_value, expected_value) in observed.iter().zip(expected.iter()) {
        assert!((observed_value - expected_value).abs() <= 1.0e-12);
    }
}

#[test]
fn sampled_cdf_values_preserve_valid_probability_mass() {
    let probability_mass = probability_mass_from_sampled_cdf_values(&[0.2, 0.5, 0.8]);

    assert_probability_mass_close(probability_mass.as_slice(), &[0.2, 0.3, 0.3, 0.2]);
}

#[test]
fn sampled_cdf_values_project_non_monotone_row_onto_probability_simplex() {
    let probability_mass = probability_mass_from_sampled_cdf_values(&[0.4, 0.2]);

    assert_probability_mass_close(probability_mass.as_slice(), &[0.3, 0.0, 0.7]);
}
