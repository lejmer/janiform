use super::super::coordinates::{FOURIER_BUSINESS_TIME_CLOCK_AXES, FourierHarmonicOrders};
use super::FourierBusinessTimeClockFitSettings;

pub(super) fn candidate_orders(
    settings: &FourierBusinessTimeClockFitSettings,
) -> Vec<CandidateOrder> {
    let Some(maximum_orders) =
        FourierHarmonicOrders::from_order_map(&settings.maximum_harmonic_orders)
    else {
        return Vec::new();
    };

    let mut candidates = Vec::new();
    let mut current_orders = FourierHarmonicOrders::zeros();

    append_candidate_orders(
        &mut candidates,
        maximum_orders,
        &mut current_orders,
        0,
        settings.maximum_parameter_count,
    );

    candidates
}

fn append_candidate_orders(
    candidates: &mut Vec<CandidateOrder>,
    maximum_orders: FourierHarmonicOrders,
    current_orders: &mut FourierHarmonicOrders,
    axis_position: usize,
    maximum_parameter_count: usize,
) {
    let Some(axis) = FOURIER_BUSINESS_TIME_CLOCK_AXES.get(axis_position).copied() else {
        let Some(parameter_count) = current_orders.parameter_count() else {
            return;
        };

        if parameter_count <= maximum_parameter_count {
            candidates.push(CandidateOrder {
                orders: *current_orders,
                parameter_count,
            });
        }

        return;
    };

    for harmonic_order in 0..=maximum_orders.order_for_axis(axis) {
        current_orders.set_order_for_axis(axis, harmonic_order);

        append_candidate_orders(
            candidates,
            maximum_orders,
            current_orders,
            axis_position + 1,
            maximum_parameter_count,
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CandidateOrder {
    pub(super) orders: FourierHarmonicOrders,
    pub(super) parameter_count: usize,
}

impl CandidateOrder {
    pub(super) fn label(self) -> String {
        self.orders
            .axis_orders()
            .map(|(axis, harmonic_order)| format!("{}_k={harmonic_order}", axis.label()))
            .collect::<Vec<_>>()
            .join(", ")
    }
}
