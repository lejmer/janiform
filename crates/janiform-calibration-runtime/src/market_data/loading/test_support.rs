pub(super) fn tick(
    timestamp_utc: &str,
) -> crate::market_data::preprocessing::MarketDataTranslationTickRecord {
    use chrono::{DateTime, Utc};

    crate::market_data::preprocessing::MarketDataTranslationTickRecord {
        timestamp_utc_nanos: DateTime::parse_from_rfc3339(timestamp_utc)
            .expect("test timestamp must parse")
            .with_timezone(&Utc)
            .timestamp_nanos_opt()
            .expect("test timestamp must fit into nanoseconds"),
        price: Some(1.0),
        trade_sign: None,
        source_row_number: 1,
    }
}
