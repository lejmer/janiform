from janiform.pricing import (
    AnchoredVanillaOptionPricingBoundaryInput,
    GammaSeverityMarkConfiguration,
    MeanReversionBoundaryState,
    MeanReversionFittedParameters,
    NigSharedPlusShiftConfiguration,
    VanillaOptionPayoffConfiguration,
    VanillaOptionPayoffType,
    VanillaOptionPricingKernelParameters,
    VanillaOptionPricingMarketInputs,
    VanillaOptionPricingModelParameters,
)
from janiform.return_clock import ReturnClockExposureParameters
from janiform.sufficient_statistics import (
    SufficientStatisticsEquationSpecification,
    SufficientStatisticsModelInputs,
)
from janiform.simulation import (
    HawkesExcitationState,
    HawkesKernelLoadings,
    HawkesKernelParameters,
    LongVarianceParameters,
    LongVarianceState,
    MarketPressureParameters,
    MarketPressureState,
    MeanReversionRuntimeIntervalAnchors,
)
from janiform.simulation.coarse import (
    KnownNewsArrivalConfiguration,
    NewsArrivalConfiguration,
    NewsComponentMarkScaleConfiguration,
    NewsProcessConfiguration,
    SharedNewsMarkLawConfiguration,
    SurpriseNewsArrivalConfiguration,
)


def build_excitation_state() -> HawkesExcitationState:
    return HawkesExcitationState(
        y_plus_plus=0.0,
        y_plus_minus=0.0,
        y_minus_plus=0.0,
        y_minus_minus=0.0,
    )


def build_kernel_parameters() -> HawkesKernelParameters:
    return HawkesKernelParameters(
        lambda_bar=0.8,
        beta=1.6,
        alpha=HawkesKernelLoadings(
            plus_plus=0.2,
            plus_minus=0.1,
            minus_plus=0.1,
            minus_minus=0.25,
        ),
        rho=HawkesKernelLoadings(
            plus_plus=0.08,
            plus_minus=0.04,
            minus_plus=0.05,
            minus_minus=0.1,
        ),
    )


def build_nig_shared_plus_shift_configuration() -> NigSharedPlusShiftConfiguration:
    return NigSharedPlusShiftConfiguration(
        alpha=2.3,
        beta_0=-0.8,
        delta=0.7,
        mu=0.1,
        delta_beta=-0.2,
    )


def build_news_process_configuration() -> NewsProcessConfiguration:
    return NewsProcessConfiguration(
        schema_version="news.process.configuration.v1",
        arrivals=NewsArrivalConfiguration(
            known=KnownNewsArrivalConfiguration(
                scheduled_event_timestamps_utc_nanos=(),
            ),
            surprise=SurpriseNewsArrivalConfiguration(
                open_baseline_intensity=0.75,
                closed_baseline_intensity=0.75,
            ),
        ),
        shared_mark_law=SharedNewsMarkLawConfiguration(
            gamma_severity_marks=GammaSeverityMarkConfiguration(
                shape_k=1.8,
            ),
        ),
        known_component_mark_scales=NewsComponentMarkScaleConfiguration(
            activity_coupling_scale=1.0,
            return_jump_drift=0.0,
            return_jump_volatility=1.0,
        ),
        surprise_component_mark_scales=NewsComponentMarkScaleConfiguration(
            activity_coupling_scale=1.0,
            return_jump_drift=0.0,
            return_jump_volatility=1.0,
        ),
    )


def build_mean_reversion_fitted_parameters() -> MeanReversionFittedParameters:
    return MeanReversionFittedParameters(kappa=0.35)


def build_market_pressure_parameters() -> MarketPressureParameters:
    return MarketPressureParameters.from_mapping(
        {
            "side_imbalance_levels": [0.0],
            "transition_rates": {
                "upward_transition_rates": [],
                "downward_transition_rates": [],
            },
            "anchor_drift_per_signed_balance": 0.0,
        }
    )


def build_market_pressure_state() -> MarketPressureState:
    return MarketPressureState(state_weights=[1.0])


def build_mean_reversion_runtime_contracts(
    *,
    interval_count: int,
    interval_delta_hours: float = 1.0,
    start_anchor_value: float = 0.25,
    observed_log_price: float = 0.25,
) -> tuple[
    MeanReversionFittedParameters,
    MeanReversionRuntimeIntervalAnchors,
    MeanReversionBoundaryState,
]:
    fitted_parameters = build_mean_reversion_fitted_parameters()
    runtime_interval_anchors = MeanReversionRuntimeIntervalAnchors(
        interval_alignment_policy="explicit_runtime_interval_grid_alignment.v1",
        start_anchor_value=start_anchor_value,
        end_anchor_values=tuple(start_anchor_value for _ in range(interval_count)),
        interval_deltas=tuple(interval_delta_hours for _ in range(interval_count)),
    )
    initial_boundary_state = MeanReversionBoundaryState(
        observed_log_price=observed_log_price,
        anchor_value=start_anchor_value,
        deviation=observed_log_price - start_anchor_value,
    )

    return fitted_parameters, runtime_interval_anchors, initial_boundary_state


def build_pricing_market_inputs() -> VanillaOptionPricingMarketInputs:
    return VanillaOptionPricingMarketInputs(
        spot=101.0,
        strikes=(100.0,),
        maturity_intervals=(1.0,),
        risk_free_rate=0.02,
        dividend_yield=0.01,
    )


def build_pricing_boundary_input() -> AnchoredVanillaOptionPricingBoundaryInput:
    return AnchoredVanillaOptionPricingBoundaryInput(
        interval_index=0,
        hawkes_boundary_state=build_excitation_state(),
        long_variance_boundary_state=LongVarianceState.unit(),
        mean_reversion_fitted_parameters=build_mean_reversion_fitted_parameters(),
        market_pressure_boundary_state=build_market_pressure_state(),
        interval_delta=1.0,
        observed_boundary_state=MeanReversionBoundaryState(
            observed_log_price=0.25,
            anchor_value=0.25,
            deviation=0.0,
        ),
    )


def build_pricing_model_parameters() -> VanillaOptionPricingModelParameters:
    return VanillaOptionPricingModelParameters(
        hawkes_kernel_parameters=build_kernel_parameters(),
        news_process_configuration=build_news_process_configuration(),
        nig_shared_plus_shift_configuration=build_nig_shared_plus_shift_configuration(),
        return_clock_exposure_parameters=ReturnClockExposureParameters.neutral(),
        long_variance_parameters=LongVarianceParameters.neutral(),
        market_pressure_parameters=build_market_pressure_parameters(),
        pricing_kernel_parameters=VanillaOptionPricingKernelParameters.identity(),
    )


def build_pricing_payoff() -> VanillaOptionPayoffConfiguration:
    return VanillaOptionPayoffConfiguration(
        payoff_type=VanillaOptionPayoffType.CALL,
    )


def build_sufficient_statistics_model_inputs(
    *,
    interval_count: int,
    interval_delta_hours: float = 1.0,
) -> SufficientStatisticsModelInputs:
    (
        mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state,
    ) = build_mean_reversion_runtime_contracts(
        interval_count=interval_count,
        interval_delta_hours=interval_delta_hours,
        start_anchor_value=0.0,
        observed_log_price=0.0,
    )

    return SufficientStatisticsModelInputs(
        equation_specification=SufficientStatisticsEquationSpecification(
            hawkes_kernel_parameters=build_kernel_parameters(),
            gamma_severity_mark_configuration=GammaSeverityMarkConfiguration(
                shape_k=1.8,
            ),
            long_variance_parameters=LongVarianceParameters.neutral(),
            market_pressure_parameters=build_market_pressure_parameters(),
        ),
        nig_shared_plus_shift_configuration=build_nig_shared_plus_shift_configuration(),
        initial_excitation_state=HawkesExcitationState(
            y_plus_plus=0.2,
            y_plus_minus=0.1,
            y_minus_plus=0.3,
            y_minus_minus=0.4,
        ),
        long_variance_parameters=LongVarianceParameters.neutral(),
        initial_long_variance_state=LongVarianceState.unit(),
        initial_market_pressure_state=build_market_pressure_state(),
        mean_reversion_fitted_parameters=mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors=mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state=initial_mean_reversion_boundary_state,
    )
