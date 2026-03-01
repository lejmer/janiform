#[test]
fn pricing_and_simulation_public_api_names_must_not_include_consumer_suffixes() {
    let pricing_sources = [
        ("pricing/mod.rs", include_str!("../src/pricing/mod.rs")),
        (
            "pricing/characteristic.rs",
            include_str!("../src/pricing/characteristic.rs"),
        ),
        (
            "pricing/constants.rs",
            include_str!("../src/pricing/constants.rs"),
        ),
        (
            "pricing/contracts.rs",
            include_str!("../src/pricing/contracts.rs"),
        ),
        (
            "pricing/fourier.rs",
            include_str!("../src/pricing/fourier.rs"),
        ),
    ];
    let simulation_source = include_str!("../src/simulation/mod.rs");

    let pricing_violations: Vec<String> = pricing_sources
        .into_iter()
        .flat_map(|(source_name, source)| collect_public_for_api_items(source_name, source))
        .collect();
    let simulation_violations =
        collect_public_for_api_items("simulation/mod.rs", simulation_source);

    assert!(
        pricing_violations.is_empty(),
        "pricing module contains public API items with `*_for_*` names:\n{}",
        pricing_violations.join("\n")
    );
    assert!(
        simulation_violations.is_empty(),
        "simulation/mod.rs contains public API items with `*_for_*` names:\n{}",
        simulation_violations.join("\n")
    );
}

fn collect_public_for_api_items(source_name: &str, source: &str) -> Vec<String> {
    let mut violations: Vec<String> = Vec::new();
    let mut search_start = 0usize;

    while let Some(relative_position) = source[search_start..].find("pub fn ") {
        let function_start = search_start + relative_position;
        let name_start = function_start + "pub fn ".len();
        let name_end = source[name_start..]
            .find(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
            .map(|offset| name_start + offset)
            .expect("public function name must terminate");
        let function_name = &source[name_start..name_end];
        if function_name.contains("_for_") {
            let line_number = source[..function_start]
                .chars()
                .filter(|c| *c == '\n')
                .count()
                + 1;
            violations.push(format!(
                "{} line {}: pub fn {}",
                source_name, line_number, function_name
            ));
        }
        search_start = name_end;
    }

    let mut pending_pub_use_statement = String::new();
    let mut pending_pub_use_start_line = 0usize;
    for (line_index, raw_line) in source.lines().enumerate() {
        let trimmed_line = raw_line.trim();

        if pending_pub_use_statement.is_empty() {
            if trimmed_line.starts_with("pub use ") {
                pending_pub_use_statement.push_str(trimmed_line);
                pending_pub_use_start_line = line_index + 1;
                if !trimmed_line.ends_with(';') {
                    pending_pub_use_statement.push(' ');
                }
            }
        } else {
            pending_pub_use_statement.push_str(trimmed_line);
            if !trimmed_line.ends_with(';') {
                pending_pub_use_statement.push(' ');
            }
        }

        if !pending_pub_use_statement.is_empty() && trimmed_line.ends_with(';') {
            if pending_pub_use_statement.contains("_for_") {
                violations.push(format!(
                    "{} line {}: {}",
                    source_name, pending_pub_use_start_line, pending_pub_use_statement
                ));
            }
            pending_pub_use_statement.clear();
            pending_pub_use_start_line = 0;
        }
    }

    violations
}
