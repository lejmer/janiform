"""Shared pricing field names used by validation and user-facing outputs."""

from typing import Final


STRIKE_FIELD_NAME: Final[str] = "strike"
OPTION_TYPE_FIELD_NAME: Final[str] = "option_type"
MATURITY_INTERVAL_FIELD_NAME: Final[str] = "maturity_interval"
IMPLIED_VOLATILITY_FIELD_NAME: Final[str] = "implied_volatility"
IMPLIED_VOLATILITY_INVERSION_STATUS_FIELD_NAME: Final[str] = "implied_volatility_inversion_status"
MODEL_PRICE_FIELD_NAME: Final[str] = "model_price"
MODEL_IMPLIED_VOLATILITY_FIELD_NAME: Final[str] = "model_implied_volatility"
MODEL_Q_IMPLIED_VOLATILITY_FIELD_NAME: Final[str] = "model_q_implied_volatility"
OBSERVED_Q_IMPLIED_VOLATILITY_FIELD_NAME: Final[str] = "observed_q_implied_volatility"
OBSERVED_Q_IMPLIED_VOLATILITY_INVERSION_STATUS_FIELD_NAME: Final[str] = (
    "observed_q_implied_volatility_inversion_status"
)
PRICING_STATUS_FIELD_NAME: Final[str] = "pricing_status"
PRICING_FAILURE_STAGE_FIELD_NAME: Final[str] = "pricing_failure_stage"
PRICING_FAILURE_REASON_FIELD_NAME: Final[str] = "pricing_failure_reason"
FORWARD_PRICE_FIELD_NAME: Final[str] = "forward_price"
Q_FORWARD_PRICE_FIELD_NAME: Final[str] = "q_forward_price"
P_SIMULATED_FORWARD_PRICE_FIELD_NAME: Final[str] = "p_simulated_forward_price"
SAMPLE_COUNT_FIELD_NAME: Final[str] = "sample_count"
PHYSICAL_FORWARD_NORMALIZED_PRICE_FIELD_NAME: Final[str] = "physical_forward_normalized_price"
PHYSICAL_FORWARD_NORMALIZED_IMPLIED_VOLATILITY_FIELD_NAME: Final[str] = (
    "physical_forward_normalized_implied_volatility"
)
PHYSICAL_FORWARD_NORMALIZED_IMPLIED_VOLATILITY_INVERSION_STATUS_FIELD_NAME: Final[str] = (
    "physical_forward_normalized_implied_volatility_inversion_status"
)
MARKET_FORWARD_PHYSICAL_PRICE_FIELD_NAME: Final[str] = "market_forward_physical_price"
MARKET_FORWARD_PHYSICAL_IMPLIED_VOLATILITY_FIELD_NAME: Final[str] = (
    "market_forward_physical_implied_volatility"
)
MARKET_FORWARD_PHYSICAL_IMPLIED_VOLATILITY_INVERSION_STATUS_FIELD_NAME: Final[str] = (
    "market_forward_physical_implied_volatility_inversion_status"
)
LOG_FORWARD_MONEYNESS_FIELD_NAME: Final[str] = "log_forward_moneyness"
DISCOUNT_FACTOR_FIELD_NAME: Final[str] = "discount_factor"
BOUNDARY_STATE_NORM_FIELD_NAME: Final[str] = "boundary_state_norm"
DIVIDEND_YIELD_FIELD_NAME: Final[str] = "dividend_yield"
EXPIRY_TIMESTAMP_UTC_FIELD_NAME: Final[str] = "expiry_timestamp_utc"
OBSERVED_ASK_PRICE_FIELD_NAME: Final[str] = "observed_ask_price"
OBSERVED_BID_PRICE_FIELD_NAME: Final[str] = "observed_bid_price"
OBSERVED_MID_PRICE_FIELD_NAME: Final[str] = "observed_mid_price"
QUOTE_TIMESTAMP_UTC_FIELD_NAME: Final[str] = "quote_timestamp_utc"
RISK_FREE_RATE_FIELD_NAME: Final[str] = "risk_free_rate"
TIME_TO_EXPIRY_DAYS_FIELD_NAME: Final[str] = "time_to_expiry_days"
TIME_TO_EXPIRY_HOURS_FIELD_NAME: Final[str] = "time_to_expiry_hours"
VALUATION_TIME_UTC_FIELD_NAME: Final[str] = "valuation_time_utc"
