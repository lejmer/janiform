from janiform.pricing import (
    AnchoredVanillaOptionPricingBoundaryInput,
    GammaSeverityMarkConfiguration,
    MeanReversionAnchorModel,
    MeanReversionBoundaryState,
    MeanReversionFittedParameters,
    NigSharedPlusShiftConfiguration,
    VanillaOptionPayoffConfiguration,
    VanillaOptionPayoffType,
    VanillaOptionPricingExecutionInput,
    VanillaOptionPricingMarketInputs,
    VanillaOptionPricingModelParameters,
    VanillaOptionPricingOptionalOutputSelection,
    VanillaOptionPricingRequest,
)
from janiform.simulation import HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters
from janiform.simulation.coarse import (
    CoupledGapMarkConfiguration,
    DoubleExponentialBaseShockConfiguration,
    KnownNewsArrivalConfiguration,
    NewsArrivalConfiguration,
    NewsComponentMarkScaleConfiguration,
    NewsProcessConfiguration,
    SharedNewsMarkLawConfiguration,
    SurpriseNewsArrivalConfiguration,
)
from tests.support.hourly_factor_contracts import build_hourly_factor_contract


def build_anchored_request_model() -> VanillaOptionPricingRequest:
    return VanillaOptionPricingRequest(
        market_inputs=VanillaOptionPricingMarketInputs(
            spot=101.0,
            strike=100.0,
            maturity_interval=1.0,
            risk_free_rate=0.02,
            dividend_yield=0.01,
        ),
        payoff=VanillaOptionPayoffConfiguration(payoff_type=VanillaOptionPayoffType.CALL),
        boundary_input=AnchoredVanillaOptionPricingBoundaryInput(
            interval_index=0,
            hawkes_boundary_state=HawkesExcitationState(
                y_plus_plus=0.0,
                y_plus_minus=0.0,
                y_minus_plus=0.0,
                y_minus_minus=0.0,
            ),
            mean_reversion_fitted_parameters=MeanReversionFittedParameters(
                kappa=0.35,
                anchor_model=MeanReversionAnchorModel.zero_gain_endogenous_core_pressure_level(),
            ),
            interval_delta=1.0,
            observed_boundary_state=MeanReversionBoundaryState(
                observed_log_price=0.4,
                anchor_value=0.25,
                pressure_state=0.0,
                deviation=0.15,
            ),
        ),
        model_parameters=_build_model_parameters(),
    )


def build_anchored_runtime_execution_input_model() -> VanillaOptionPricingExecutionInput:
    anchored_request = build_anchored_request_model()

    return VanillaOptionPricingExecutionInput(
        request=VanillaOptionPricingRequest(
            market_inputs=anchored_request.market_inputs,
            payoff=anchored_request.payoff,
            boundary_input=AnchoredVanillaOptionPricingBoundaryInput(
                interval_index=0,
                hawkes_boundary_state=HawkesExcitationState(
                    y_plus_plus=0.0,
                    y_plus_minus=0.0,
                    y_minus_plus=0.0,
                    y_minus_minus=0.0,
                ),
                mean_reversion_fitted_parameters=MeanReversionFittedParameters(
                    kappa=0.35,
                    anchor_model=MeanReversionAnchorModel.zero_gain_endogenous_core_pressure_level(),
                ),
                interval_delta=1.0,
                observed_boundary_state=MeanReversionBoundaryState(
                    observed_log_price=0.25,
                    anchor_value=0.25,
                    pressure_state=0.0,
                    deviation=0.0,
                ),
            ),
            model_parameters=anchored_request.model_parameters,
        ),
        hourly_factor_contract=build_hourly_factor_contract(
            request_start_utc_nanos=1_706_882_400_000_000_000,
            s_h=(1.0,),
            known_component_nu_h=(0.0,),
            surprise_component_nu_h=(0.0,),
        ),
        optional_output_selection=VanillaOptionPricingOptionalOutputSelection(
            include_transform_diagnostics=True,
            include_integration_quality_diagnostics=True,
            include_calibration_hooks=True,
        ),
    )


def _build_model_parameters() -> VanillaOptionPricingModelParameters:
    return VanillaOptionPricingModelParameters(
        hawkes_kernel_parameters=HawkesKernelParameters(
            lambda_bar=0.8,
            beta=1.6,
            alpha=HawkesKernelLoadings(
                plus_plus=0.2,
                plus_minus=0.1,
                minus_plus=0.1,
                minus_minus=0.25,
            ),
            rho=HawkesKernelLoadings(
                plus_plus=0.1,
                plus_minus=0.05,
                minus_plus=0.05,
                minus_minus=0.1,
            ),
        ),
        news_process_configuration=_build_news_process_configuration(),
        nig_shared_plus_shift_configuration=NigSharedPlusShiftConfiguration(
            alpha=2.3,
            beta_0=-0.8,
            delta=0.7,
            mu=0.1,
            delta_beta=-0.2,
        ),
    )


def _build_news_process_configuration() -> NewsProcessConfiguration:
    return NewsProcessConfiguration(
        schema_version="news.process.configuration.v1",
        arrivals=NewsArrivalConfiguration(
            known=KnownNewsArrivalConfiguration(
                scheduled_event_timestamps_utc_nanos=(),
            ),
            surprise=SurpriseNewsArrivalConfiguration(
                baseline_intensity=0.75,
            ),
        ),
        shared_mark_law=SharedNewsMarkLawConfiguration(
            gamma_severity_marks=GammaSeverityMarkConfiguration(
                shape_k=1.8,
                scale_theta=0.7,
            ),
            double_exponential_base_shock=DoubleExponentialBaseShockConfiguration(
                probability_positive=0.35,
                eta_plus=2.1,
                eta_minus=3.0,
            ),
            coupled_gap_marks=CoupledGapMarkConfiguration(
                sigma_g=0.6,
            ),
        ),
        known_component_mark_scales=NewsComponentMarkScaleConfiguration(
            activity_coupling_scale=0.0,
            return_jump_scale=1.0,
        ),
        surprise_component_mark_scales=NewsComponentMarkScaleConfiguration(
            activity_coupling_scale=0.0,
            return_jump_scale=1.0,
        ),
    )
