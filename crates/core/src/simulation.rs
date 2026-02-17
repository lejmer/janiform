//! Simulation-side hourly factor loading through the shared provider interface.

use crate::calendar::ExchangeCalendarRegistry;
use crate::hourly_factor_provider::{
    HourlyFactorProviderError, HourlyFactorProviderRequest, provide_hourly_factor_contract,
};
use crate::hourly_factors::HourlyFactorContractOutput;

/// Resolve interval-aligned hourly factors for the simulation path.
pub fn load_hourly_factor_contract_for_simulation(
    calendar_registry: &ExchangeCalendarRegistry,
    provider_request: HourlyFactorProviderRequest<'_>,
) -> Result<HourlyFactorContractOutput, HourlyFactorProviderError> {
    provide_hourly_factor_contract(calendar_registry, provider_request)
}
