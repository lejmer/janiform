use super::FourierBusinessTimeClockFitSettings;

pub(super) fn candidate_orders(
    settings: FourierBusinessTimeClockFitSettings,
) -> Vec<CandidateOrder> {
    let mut candidates = Vec::new();

    for intraday_harmonic_order in 0..=settings.maximum_intraday_harmonic_order {
        for weekly_harmonic_order in 0..=settings.maximum_weekly_harmonic_order {
            for annual_harmonic_order in 0..=settings.maximum_annual_harmonic_order {
                let parameter_count = parameter_count(
                    intraday_harmonic_order,
                    weekly_harmonic_order,
                    annual_harmonic_order,
                );
                if parameter_count <= settings.maximum_parameter_count {
                    candidates.push(CandidateOrder {
                        intraday_harmonic_order,
                        weekly_harmonic_order,
                        annual_harmonic_order,
                        parameter_count,
                    });
                }
            }
        }
    }

    candidates
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CandidateOrder {
    pub(super) intraday_harmonic_order: usize,
    pub(super) weekly_harmonic_order: usize,
    pub(super) annual_harmonic_order: usize,
    pub(super) parameter_count: usize,
}

impl CandidateOrder {
    pub(super) fn label(self) -> String {
        format!(
            "intraday_k={}, weekly_k={}, annual_k={}",
            self.intraday_harmonic_order, self.weekly_harmonic_order, self.annual_harmonic_order
        )
    }
}

fn parameter_count(
    intraday_harmonic_order: usize,
    weekly_harmonic_order: usize,
    annual_harmonic_order: usize,
) -> usize {
    1 + 2 * (intraday_harmonic_order + weekly_harmonic_order + annual_harmonic_order)
}
