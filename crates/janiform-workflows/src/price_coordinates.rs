use serde_json::Value;

pub(crate) fn price_coordinate_resolution_summary(resolution: &Value) -> Option<String> {
    if resolution.is_null() {
        return None;
    }

    let dynamics_fit = resolution.get("dynamics_fit")?;
    let option_valuation = resolution.get("option_valuation")?;
    let parity_check = resolution.get("parity_forward_check");

    let mut rows = vec![
        format!(
            "Dynamics fit    : {} price, {}",
            coordinate_kind_label(dynamics_fit),
            tick_mapping_summary(dynamics_fit),
        ),
        format!(
            "Option valuation: {} price, {}",
            coordinate_kind_label(option_valuation),
            tick_mapping_summary(option_valuation),
        ),
    ];

    if let Some(underlier) = resolution
        .get("option_underlier_identity")
        .and_then(Value::as_str)
    {
        rows.push(format!("Option underlier: {underlier}"));
    }

    if let Some(parity_check) = parity_check
        && !parity_check.is_null()
    {
        rows.push(format!(
            "Parity forward  : {}",
            parity_check_summary(parity_check)
        ));
    }

    Some(rows.join("\n"))
}

fn coordinate_kind_label(coordinate: &Value) -> &'static str {
    match coordinate
        .get("coordinate_kind")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
    {
        "continuous" => "adjusted continuous",
        "raw_contract" => "raw contract",
        "single" => "single coordinate",
        _ => "unknown coordinate",
    }
}

fn tick_mapping_summary(coordinate: &Value) -> String {
    let Some(mapping) = coordinate.get("tick_data_column_mapping") else {
        return "columns unavailable".to_string();
    };

    let price = mapping
        .get("price_column")
        .and_then(Value::as_str)
        .unwrap_or("-");

    let bid = mapping
        .get("bid_column")
        .and_then(Value::as_str)
        .unwrap_or("-");

    let ask = mapping
        .get("ask_column")
        .and_then(Value::as_str)
        .unwrap_or("-");

    format!("price={price}, bid={bid}, ask={ask}")
}

fn parity_check_summary(parity_check: &Value) -> String {
    let model = optional_number_label(parity_check.get("model_forward_price"));
    let observed = optional_number_label(parity_check.get("observed_forward_price"));
    let delta = optional_number_label(parity_check.get("absolute_delta"));

    let relative_delta = parity_check
        .get("relative_delta")
        .and_then(Value::as_f64)
        .map(|value| format!("{:.4}%", value * 100.0))
        .unwrap_or_else(|| "-".to_string());

    let status = parity_check
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");

    format!(
        "model={model}, observed={observed}, delta={delta}, relative delta={relative_delta}, status={status}"
    )
}

fn optional_number_label(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_f64)
        .map(|number| format!("{number:.6}"))
        .unwrap_or_else(|| "-".to_string())
}
