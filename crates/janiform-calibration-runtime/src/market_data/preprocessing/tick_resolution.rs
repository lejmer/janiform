use super::contracts::{MarketDataTranslationPreprocessingError, MarketDataTranslationTickRecord};
use super::session_windows::resolve_market_state_and_session_window;
use super::shared::{
    CalibrationTimestampEventRecord, ExchangeCalendarDefinition, MarketState, OpenTickRecord,
    ResolvedTickRecord, TickPreprocessingSummary, TradeSign, Tz, build_source_window,
    datetime_from_nanos, parse_trade_sign,
};

pub(super) fn preprocess_tick_records(
    tick_records: &[MarketDataTranslationTickRecord],
    definition: &ExchangeCalendarDefinition,
    timezone: Tz,
    exchange_code: &str,
) -> Result<TickPreprocessingSummary, MarketDataTranslationPreprocessingError> {
    let mut resolved_ticks: Vec<ResolvedTickRecord> = Vec::with_capacity(tick_records.len());
    let mut open_price_ticks: Vec<OpenTickRecord> = Vec::new();

    let mut timestamp_event_records: Vec<CalibrationTimestampEventRecord> =
        Vec::with_capacity(tick_records.len());

    let mut session_open_tick_count: usize = 0;
    let mut session_closed_tick_count: usize = 0;
    let mut open_session_buy_tick_count: usize = 0;
    let mut open_session_sell_tick_count: usize = 0;
    let mut open_signed_tick_count: usize = 0;

    for tick_record in tick_records {
        let timestamp_utc = datetime_from_nanos(
            tick_record.timestamp_utc_nanos,
            MarketDataTranslationPreprocessingError::TickTimestampOutOfRange {
                source_row_number: tick_record.source_row_number,
            },
        )?;

        let trade_sign = parse_trade_sign(tick_record.trade_sign.as_deref())?;

        let (market_state, session_window) = resolve_market_state_and_session_window(
            definition,
            timezone,
            exchange_code,
            timestamp_utc,
        )?;

        timestamp_event_records.push(CalibrationTimestampEventRecord {
            source_record_position: tick_record.source_row_number.saturating_sub(1) as u64,
            timestamp_utc_nanos: tick_record.timestamp_utc_nanos,
        });

        match market_state {
            MarketState::Open => {
                session_open_tick_count += 1;

                if let Some(sign) = trade_sign {
                    open_signed_tick_count += 1;

                    match sign {
                        TradeSign::Buy => open_session_buy_tick_count += 1,
                        TradeSign::Sell => open_session_sell_tick_count += 1,
                    }
                }

                if let Some(price) = tick_record.price {
                    if price <= 0.0 {
                        return Err(
                            MarketDataTranslationPreprocessingError::InvalidOpenSessionPrice {
                                source_row_number: tick_record.source_row_number,
                                price,
                            },
                        );
                    }

                    let Some(session_window) = session_window else {
                        return Err(
                            MarketDataTranslationPreprocessingError::InvalidSessionBoundary {
                                exchange_code: exchange_code.to_string(),
                                timestamp_utc_nanos: tick_record.timestamp_utc_nanos,
                            },
                        );
                    };

                    open_price_ticks.push(OpenTickRecord {
                        timestamp_utc_nanos: tick_record.timestamp_utc_nanos,
                        price,
                        trade_sign,
                        session_open_utc_nanos: session_window.open_utc_nanos,
                        session_close_utc_nanos: session_window.close_utc_nanos,
                    });
                }
            }

            MarketState::Closed => {
                session_closed_tick_count += 1;
            }
        }

        resolved_ticks.push(ResolvedTickRecord {
            timestamp_utc_nanos: tick_record.timestamp_utc_nanos,
            trade_sign,
            source_row_number: tick_record.source_row_number,
        });
    }

    let source_window = build_source_window(tick_records);

    Ok(TickPreprocessingSummary {
        resolved_ticks,
        open_price_ticks,
        timestamp_event_records,
        source_window,
        session_open_tick_count,
        session_closed_tick_count,
        open_session_buy_tick_count,
        open_session_sell_tick_count,
        open_signed_tick_count,
    })
}
