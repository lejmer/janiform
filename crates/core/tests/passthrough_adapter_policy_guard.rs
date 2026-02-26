#[test]
fn pricing_and_simulation_public_api_names_must_not_include_consumer_suffixes() {
    let pricing_source = include_str!("../src/pricing.rs");
    let simulation_source = include_str!("../src/simulation.rs");

    let pricing_violations = collect_public_for_api_items(pricing_source);
    let simulation_violations = collect_public_for_api_items(simulation_source);

    assert!(
        pricing_violations.is_empty(),
        "pricing.rs contains public API items with `*_for_*` names:\n{}",
        pricing_violations.join("\n")
    );
    assert!(
        simulation_violations.is_empty(),
        "simulation.rs contains public API items with `*_for_*` names:\n{}",
        simulation_violations.join("\n")
    );
}

fn collect_public_for_api_items(source: &str) -> Vec<String> {
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
            violations.push(format!("line {}: pub fn {}", line_number, function_name));
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
                    "line {}: {}",
                    pending_pub_use_start_line, pending_pub_use_statement
                ));
            }
            pending_pub_use_statement.clear();
            pending_pub_use_start_line = 0;
        }
    }

    violations
}
