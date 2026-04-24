use crate::news_loading_projection::project_nonnegative_exogenous_news_loading;

#[test]
fn exogenous_news_loading_projection_uses_both_target_equations() {
    let projected_loading =
        project_nonnegative_exogenous_news_loading(5.0, 11.0, 1.0, 3.0, 2.0, 4.0)
            .expect("projection should succeed");

    assert_eq!(projected_loading, 2.0);
}

#[test]
fn exogenous_news_loading_projection_avoids_tiny_first_coefficient_blowup() {
    let projected_loading =
        project_nonnegative_exogenous_news_loading(1.0, 2.0, 0.0, 0.0, f64::MIN_POSITIVE, 1.0)
            .expect("projection should remain finite");

    assert_eq!(projected_loading, 2.0);
}

#[test]
fn exogenous_news_loading_projection_clamps_negative_solution_to_zero() {
    let projected_loading =
        project_nonnegative_exogenous_news_loading(0.0, -2.0, 0.0, 0.0, 0.5, 1.0)
            .expect("projection should succeed");

    assert_eq!(projected_loading, 0.0);
}
