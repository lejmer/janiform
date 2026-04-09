from janiform.calibration.presets import (
    KnownNewsPresetReference,
    MarketProfileReference,
    PresetRegistry,
    PresetReferenceUsageMode,
    PresetRegistryScope,
    ResolvedPresetWorkflowConfiguration,
    SessionPresetReference,
    resolve_preset_workflow_configuration,
)


def main() -> None:
    resolved_configuration: ResolvedPresetWorkflowConfiguration = resolve_preset_workflow_configuration(
        registry=PresetRegistry(),
        market_profile_reference=MarketProfileReference(
            usage_mode=PresetReferenceUsageMode.NAMED,
            registry_scope=PresetRegistryScope.BUILT_IN,
            preset_name="eurusd_spot",
        ),
        session_reference=SessionPresetReference(
            usage_mode=PresetReferenceUsageMode.NAMED,
            registry_scope=PresetRegistryScope.BUILT_IN,
            preset_name="fx_spot_24_5",
        ),
        known_news_reference=KnownNewsPresetReference(
            usage_mode=PresetReferenceUsageMode.NAMED,
            registry_scope=PresetRegistryScope.BUILT_IN,
            preset_name="eu_us_macro_fx_core",
        ),
    )

    print(
        "resolved workflow:",
        {
            "market_profile": resolved_configuration.market_profile.selected_preset.preset_name,
            "session": resolved_configuration.session.selected_preset.preset_name,
            "session_exchange_code": resolved_configuration.session.configuration.exchange_code,
            "session_timezone": resolved_configuration.session.configuration.timezone_name,
            "known_news": resolved_configuration.known_news.selected_preset.preset_name,
            "diagnostic_codes": [
                diagnostic.code
                for diagnostic in resolved_configuration.diagnostics
            ],
        },
    )


if __name__ == "__main__":
    main()
