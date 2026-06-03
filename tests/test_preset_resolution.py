from datetime import UTC, datetime

from janiform.calibration.market_data import infer_bundled_market_profile_for_tick_data_path
from janiform.calibration.presets import (
    KnownNewsConfigurationOverridePatch,
    KnownNewsEvent,
    KnownNewsExplicitConfiguration,
    KnownNewsPresetReference,
    MarketProfileExplicitConfiguration,
    MarketProfileKnownNewsPolicy,
    MarketProfileOverridePatch,
    MarketProfileReference,
    PresetReferenceUsageMode,
    PresetRegistry,
    PresetRegistryScope,
    ResolvedWorkflowComponentSource,
    SessionConfigurationOverridePatch,
    SessionExplicitConfiguration,
    SessionPresetReference,
    resolve_preset_workflow_configuration,
)
from janiform.calibration.sessions import (
    MarketDataSessionRecurringClosedWindow,
    MarketDataSessionRegularWindow,
    MarketDataSessionResolution,
)


def test_market_profile_inference_uses_embedded_instrument_token() -> None:
    inferred_profile = infer_bundled_market_profile_for_tick_data_path(
        "Synthetic_EURUSD_Data_1M_ticks.parquet"
    )

    assert inferred_profile is not None
    assert inferred_profile["profile_name"] == "eurusd_spot"
    assert inferred_profile["session_preset_name"] == "fx_spot_24_5"
    assert inferred_profile["exchange_code"] == "FXSPOT"
    assert inferred_profile["tick_size"] == 0.00001


def test_market_profile_inference_accepts_separated_major_fx_pair_tokens() -> None:
    inferred_profile = infer_bundled_market_profile_for_tick_data_path(
        "broker_USD-JPY_1M_ticks.parquet"
    )

    assert inferred_profile is not None
    assert inferred_profile["profile_name"] == "usdjpy_spot"
    assert inferred_profile["session_preset_name"] == "fx_spot_24_5"
    assert inferred_profile["exchange_code"] == "FXSPOT"
    assert inferred_profile["known_news_preset_name"] == "none"
    assert inferred_profile["tick_size"] == 0.001


def test_market_profile_inference_accepts_two_character_futures_root() -> None:
    inferred_profile = infer_bundled_market_profile_for_tick_data_path(
        "ES_continuous_ticks.parquet"
    )

    assert inferred_profile is not None
    assert inferred_profile["profile_name"] == "es_futures_globex"
    assert inferred_profile["session_preset_name"] == "XCME"
    assert inferred_profile["exchange_code"] == "XCME"
    assert inferred_profile["tick_size"] == 0.25


def test_resolution_uses_inferred_session_and_keeps_known_news_none_for_profile_suggestions() -> None:
    resolved_configuration = resolve_preset_workflow_configuration(
        registry=PresetRegistry(),
        market_profile_reference=MarketProfileReference(
            usage_mode=PresetReferenceUsageMode.NAMED,
            registry_scope=PresetRegistryScope.BUILT_IN,
            preset_name="us_equity_cash",
        ),
        inferred_session_resolution=_build_inferred_session_resolution(),
    )

    assert resolved_configuration.market_profile is not None
    assert resolved_configuration.market_profile.selected_preset is not None
    assert resolved_configuration.market_profile.selected_preset.preset_name == "us_equity_cash"
    assert resolved_configuration.market_profile.source_kind == ResolvedWorkflowComponentSource.BUILT_IN
    assert resolved_configuration.market_profile.configuration.tick_size is None

    assert resolved_configuration.session.source_kind == ResolvedWorkflowComponentSource.INFERRED
    assert resolved_configuration.session.inferred_from_data is True
    assert resolved_configuration.session.selected_preset is None
    assert resolved_configuration.session.configuration.timezone_name == "America/New_York"

    assert resolved_configuration.known_news.source_kind == ResolvedWorkflowComponentSource.BUILT_IN
    assert resolved_configuration.known_news.selected_preset is not None
    assert resolved_configuration.known_news.selected_preset.preset_name == "none"
    assert resolved_configuration.known_news.recommended_reference is not None
    assert resolved_configuration.known_news.recommended_reference.preset_name == "us_macro_core"
    assert resolved_configuration.known_news.recommendation_applied is False

    diagnostic_codes = {diagnostic.code for diagnostic in resolved_configuration.diagnostics}
    assert "profile_component_resolution_outcome" in diagnostic_codes


def test_resolution_applies_named_override_patches_per_component() -> None:
    resolved_configuration = resolve_preset_workflow_configuration(
        registry=PresetRegistry(),
        session_reference=SessionPresetReference(
            usage_mode=PresetReferenceUsageMode.NAMED,
            registry_scope=PresetRegistryScope.BUILT_IN,
            preset_name="XNYS",
            override_patch=SessionConfigurationOverridePatch(
                timezone_name="UTC",
            ),
        ),
        known_news_reference=KnownNewsPresetReference(
            usage_mode=PresetReferenceUsageMode.NAMED,
            registry_scope=PresetRegistryScope.BUILT_IN,
            preset_name="none",
            override_patch=KnownNewsConfigurationOverridePatch(
                additional_events=(
                    KnownNewsEvent(
                        event_id="desk_macro_2026_03_18",
                        event_name="Desk Macro Watch",
                        scheduled_at=datetime(2026, 3, 18, 18, 0, tzinfo=UTC),
                        tags=("desk", "macro"),
                    ),
                ),
            ),
        ),
    )

    assert resolved_configuration.session.source_kind == ResolvedWorkflowComponentSource.OVERRIDDEN
    assert resolved_configuration.session.selected_preset is not None
    assert resolved_configuration.session.selected_preset.preset_name == "XNYS"
    assert resolved_configuration.session.applied_override_fields == ("timezone_name",)
    assert resolved_configuration.session.configuration.timezone_name == "UTC"

    assert resolved_configuration.known_news.source_kind == ResolvedWorkflowComponentSource.OVERRIDDEN
    assert resolved_configuration.known_news.selected_preset is not None
    assert resolved_configuration.known_news.selected_preset.preset_name == "none"
    assert resolved_configuration.known_news.applied_override_fields == ("additional_events",)
    assert resolved_configuration.known_news.configuration.events[0].event_id == "desk_macro_2026_03_18"


def test_resolution_applies_named_market_profile_tick_size_override_patch() -> None:
    resolved_configuration = resolve_preset_workflow_configuration(
        registry=PresetRegistry(),
        market_profile_reference=MarketProfileReference(
            usage_mode=PresetReferenceUsageMode.NAMED,
            registry_scope=PresetRegistryScope.BUILT_IN,
            preset_name="us_equity_cash",
            override_patch=MarketProfileOverridePatch(tick_size=0.01),
        ),
    )

    assert resolved_configuration.market_profile is not None
    assert resolved_configuration.market_profile.source_kind == ResolvedWorkflowComponentSource.OVERRIDDEN
    assert resolved_configuration.market_profile.applied_override_fields == ("tick_size",)
    assert resolved_configuration.market_profile.configuration.tick_size == 0.01


def test_resolution_supports_full_explicit_replacements_for_market_profile_and_components() -> None:
    resolved_configuration = resolve_preset_workflow_configuration(
        registry=PresetRegistry(),
        market_profile_reference=MarketProfileReference(
            usage_mode=PresetReferenceUsageMode.EXPLICIT,
            configuration=MarketProfileExplicitConfiguration(
                profile_name="desk_us_equity",
                session_reference=SessionPresetReference(
                    usage_mode=PresetReferenceUsageMode.NAMED,
                    registry_scope=PresetRegistryScope.BUILT_IN,
                    preset_name="XNYS",
                ),
                known_news_reference=KnownNewsPresetReference(
                    usage_mode=PresetReferenceUsageMode.NAMED,
                    registry_scope=PresetRegistryScope.BUILT_IN,
                    preset_name="us_macro_core",
                ),
                known_news_policy=MarketProfileKnownNewsPolicy.APPLY_BY_DEFAULT,
                timestamp_timezone_name="America/New_York",
                tick_size=0.01,
                default_calibration_hint_ids=("desk_equity_defaults",),
            ),
        ),
        session_reference=SessionPresetReference(
            usage_mode=PresetReferenceUsageMode.EXPLICIT,
            configuration=SessionExplicitConfiguration(
                exchange_code="XDESK",
                exchange_name="Desk Exchange",
                timezone_name="UTC",
                regular_session_windows=(
                    MarketDataSessionRegularWindow(
                        weekday="Monday",
                        label="desk",
                        open_time="08:00",
                        close_time="16:00",
                    ),
                ),
            ),
        ),
        known_news_reference=KnownNewsPresetReference(
            usage_mode=PresetReferenceUsageMode.EXPLICIT,
            configuration=KnownNewsExplicitConfiguration(
                calendar_timezone_name="UTC",
                events=(
                    KnownNewsEvent(
                        event_id="desk_cpi_2026_04",
                        event_name="Desk CPI",
                        scheduled_at=datetime(2026, 4, 10, 12, 30, tzinfo=UTC),
                        tags=("desk",),
                    ),
                ),
            ),
        ),
    )

    assert resolved_configuration.market_profile is not None
    assert resolved_configuration.market_profile.source_kind == ResolvedWorkflowComponentSource.FULLY_CUSTOM
    assert resolved_configuration.market_profile.selected_preset is None
    assert resolved_configuration.market_profile.configuration.tick_size == 0.01

    assert resolved_configuration.session.source_kind == ResolvedWorkflowComponentSource.FULLY_CUSTOM
    assert resolved_configuration.session.selected_preset is None
    assert resolved_configuration.session.configuration.exchange_code == "XDESK"

    assert resolved_configuration.known_news.source_kind == ResolvedWorkflowComponentSource.FULLY_CUSTOM
    assert resolved_configuration.known_news.selected_preset is None
    assert resolved_configuration.known_news.configuration.events[0].event_id == "desk_cpi_2026_04"


def test_resolution_records_low_confidence_and_ambiguity_diagnostics_for_inference() -> None:
    resolved_configuration = resolve_preset_workflow_configuration(
        registry=PresetRegistry(),
        inferred_session_resolution=_build_inferred_session_resolution(
            confidence_score=0.55,
            confidence_label="low",
            ambiguity_warnings=(
                "Observed gaps only partially matched the inferred closed windows.",
            ),
            metadata_discrepancies=(
                "Session timezone metadata differed from the inferred baseline.",
            ),
        ),
    )

    diagnostic_codes = [diagnostic.code for diagnostic in resolved_configuration.diagnostics]

    assert resolved_configuration.session.source_kind == ResolvedWorkflowComponentSource.INFERRED
    assert "session_inference_low_confidence" in diagnostic_codes
    assert "session_inference_ambiguous" in diagnostic_codes
    assert "session_inference_metadata_discrepancy" in diagnostic_codes


def test_resolution_does_not_flag_explicit_automatic_session_selection_as_conflict() -> None:
    resolved_configuration = resolve_preset_workflow_configuration(
        registry=PresetRegistry(),
        session_reference=SessionPresetReference(
            usage_mode=PresetReferenceUsageMode.AUTOMATIC,
        ),
        inferred_session_resolution=_build_inferred_session_resolution(),
    )

    diagnostic_codes = [diagnostic.code for diagnostic in resolved_configuration.diagnostics]

    assert resolved_configuration.session.source_kind == ResolvedWorkflowComponentSource.INFERRED
    assert resolved_configuration.session.inferred_from_data is True
    assert diagnostic_codes == []


def test_resolution_emits_conflicting_override_diagnostic_when_known_news_replaces_profile_default() -> None:
    resolved_configuration = resolve_preset_workflow_configuration(
        registry=PresetRegistry(),
        market_profile_reference=MarketProfileReference(
            usage_mode=PresetReferenceUsageMode.EXPLICIT,
            configuration=MarketProfileExplicitConfiguration(
                profile_name="desk_apply_news",
                session_reference=SessionPresetReference(
                    usage_mode=PresetReferenceUsageMode.NAMED,
                    registry_scope=PresetRegistryScope.BUILT_IN,
                    preset_name="XNYS",
                ),
                known_news_reference=KnownNewsPresetReference(
                    usage_mode=PresetReferenceUsageMode.NAMED,
                    registry_scope=PresetRegistryScope.BUILT_IN,
                    preset_name="us_macro_core",
                ),
                known_news_policy=MarketProfileKnownNewsPolicy.APPLY_BY_DEFAULT,
            ),
        ),
        known_news_reference=KnownNewsPresetReference(
            usage_mode=PresetReferenceUsageMode.NAMED,
            registry_scope=PresetRegistryScope.BUILT_IN,
            preset_name="none",
        ),
    )

    diagnostic_codes = [diagnostic.code for diagnostic in resolved_configuration.diagnostics]

    assert resolved_configuration.known_news.selected_preset is not None
    assert resolved_configuration.known_news.selected_preset.preset_name == "none"
    assert resolved_configuration.known_news.recommended_reference is not None
    assert resolved_configuration.known_news.recommended_reference.preset_name == "us_macro_core"
    assert resolved_configuration.known_news.recommendation_applied is False
    assert "conflicting_override" in diagnostic_codes


def test_resolution_does_not_flag_matching_known_news_selection_as_conflict() -> None:
    resolved_configuration = resolve_preset_workflow_configuration(
        registry=PresetRegistry(),
        market_profile_reference=MarketProfileReference(
            usage_mode=PresetReferenceUsageMode.EXPLICIT,
            configuration=MarketProfileExplicitConfiguration(
                profile_name="desk_apply_news",
                session_reference=SessionPresetReference(
                    usage_mode=PresetReferenceUsageMode.NAMED,
                    registry_scope=PresetRegistryScope.BUILT_IN,
                    preset_name="XNYS",
                ),
                known_news_reference=KnownNewsPresetReference(
                    usage_mode=PresetReferenceUsageMode.NAMED,
                    registry_scope=PresetRegistryScope.BUILT_IN,
                    preset_name="us_macro_core",
                ),
                known_news_policy=MarketProfileKnownNewsPolicy.APPLY_BY_DEFAULT,
            ),
        ),
        known_news_reference=KnownNewsPresetReference(
            usage_mode=PresetReferenceUsageMode.NAMED,
            registry_scope=PresetRegistryScope.BUILT_IN,
            preset_name="us_macro_core",
        ),
    )

    diagnostic_codes = [diagnostic.code for diagnostic in resolved_configuration.diagnostics]

    assert resolved_configuration.known_news.selected_preset is not None
    assert resolved_configuration.known_news.selected_preset.preset_name == "us_macro_core"
    assert diagnostic_codes == []


def _build_inferred_session_resolution(
    *,
    confidence_score: float = 0.92,
    confidence_label: str = "high",
    ambiguity_warnings: tuple[str, ...] = (),
    metadata_discrepancies: tuple[str, ...] = (),
) -> MarketDataSessionResolution:
    return MarketDataSessionResolution(
        schema_version="calibration.market_data.session_resolution.v1",
        resolution_kind="automatic_inference",
        provenance_kind="bundled_preset",
        selected_policy_id="exchange_code_builtin_default.xnys_regular_session.v1",
        manual_override_applied=False,
        exchange_code="XNYS",
        exchange_name="New York Stock Exchange",
        resolved_timezone_name="America/New_York",
        timezone_source="exchange_code_builtin_default",
        open_session_duration_hours_per_week=32.5,
        confidence_score=confidence_score,
        confidence_label=confidence_label,
        regular_session_windows=tuple(
            MarketDataSessionRegularWindow(
                weekday=weekday,
                label="regular",
                open_time="09:30",
                close_time="16:00",
            )
            for weekday in ("Monday", "Tuesday", "Wednesday", "Thursday", "Friday")
        ),
        recurring_closed_windows=(
            MarketDataSessionRecurringClosedWindow(
                start_weekday="Friday",
                start_time="16:00",
                end_weekday="Monday",
                end_time="09:30",
                reason="weekend_close",
            ),
        ),
        reasons=("Used the built-in XNYS exchange schedule.",),
        ambiguity_warnings=ambiguity_warnings,
        metadata_discrepancies=metadata_discrepancies,
        resolved_session_definition_json='{"schema_version":"calendar.registry.v1"}',
    )
