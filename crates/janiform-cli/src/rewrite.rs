use std::ffi::OsStr;
use std::ffi::OsString;

pub(crate) fn rewrite_beginner_fit_arguments(arguments: Vec<OsString>) -> Vec<OsString> {
    if arguments.len() < 3 || arguments.get(1).map(OsString::as_os_str) != Some(OsStr::new("fit")) {
        return arguments;
    }

    match arguments.get(2).map(OsString::as_os_str) {
        Some(command) if command == OsStr::new("inspect") => {
            rewrite_fit_inspect_arguments(arguments)
        }
        Some(command)
            if command == OsStr::new("plan")
                || command == OsStr::new("staged")
                || command.as_encoded_bytes().starts_with(b"-") =>
        {
            arguments
        }
        Some(_) => rewrite_fit_arguments(arguments),
        None => arguments,
    }
}

fn rewrite_fit_arguments(arguments: Vec<OsString>) -> Vec<OsString> {
    let mut rewritten_arguments = Vec::with_capacity(arguments.len() + 1);
    let mut argument_iterator = arguments.into_iter();

    if let Some(program_name) = argument_iterator.next() {
        rewritten_arguments.push(program_name);
    }
    if let Some(fit_command) = argument_iterator.next() {
        rewritten_arguments.push(fit_command);
    }
    if let Some(tick_data_path) = argument_iterator.next() {
        rewritten_arguments.push(OsString::from("--tick-data-path"));
        rewritten_arguments.push(tick_data_path);
    }

    rewritten_arguments.extend(argument_iterator);
    rewritten_arguments
}

fn rewrite_fit_inspect_arguments(arguments: Vec<OsString>) -> Vec<OsString> {
    if arguments.len() < 4 {
        return arguments;
    }

    match arguments.get(3).map(OsString::as_os_str) {
        Some(command) if command.as_encoded_bytes().starts_with(b"-") => arguments,
        Some(_) => {
            let mut rewritten_arguments = Vec::with_capacity(arguments.len() + 1);
            let mut argument_iterator = arguments.into_iter();

            if let Some(program_name) = argument_iterator.next() {
                rewritten_arguments.push(program_name);
            }
            if let Some(fit_command) = argument_iterator.next() {
                rewritten_arguments.push(fit_command);
            }
            if let Some(inspect_command) = argument_iterator.next() {
                rewritten_arguments.push(inspect_command);
            }
            if let Some(tick_data_path) = argument_iterator.next() {
                rewritten_arguments.push(OsString::from("--tick-data-path"));
                rewritten_arguments.push(tick_data_path);
            }

            rewritten_arguments.extend(argument_iterator);
            rewritten_arguments
        }
        None => arguments,
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::rewrite_beginner_fit_arguments;

    fn rewrite(arguments: &[&str]) -> Vec<OsString> {
        rewrite_beginner_fit_arguments(arguments.iter().map(OsString::from).collect())
    }

    #[test]
    fn rewrites_fit_path_to_tick_data_path() {
        assert_eq!(
            rewrite(&["janiform", "fit", "ticks.parquet", "--run-id", "es"]),
            vec![
                OsString::from("janiform"),
                OsString::from("fit"),
                OsString::from("--tick-data-path"),
                OsString::from("ticks.parquet"),
                OsString::from("--run-id"),
                OsString::from("es"),
            ],
        );
    }

    #[test]
    fn rewrites_fit_inspect_path_to_tick_data_path() {
        assert_eq!(
            rewrite(&["janiform", "fit", "inspect", "ticks.parquet"]),
            vec![
                OsString::from("janiform"),
                OsString::from("fit"),
                OsString::from("inspect"),
                OsString::from("--tick-data-path"),
                OsString::from("ticks.parquet"),
            ],
        );
    }

    #[test]
    fn does_not_rewrite_fit_plan_or_staged_paths() {
        assert_eq!(
            rewrite(&["janiform", "fit", "plan", "ticks.parquet"]),
            vec![
                OsString::from("janiform"),
                OsString::from("fit"),
                OsString::from("plan"),
                OsString::from("ticks.parquet"),
            ],
        );
        assert_eq!(
            rewrite(&["janiform", "fit", "staged", "input.json"]),
            vec![
                OsString::from("janiform"),
                OsString::from("fit"),
                OsString::from("staged"),
                OsString::from("input.json"),
            ],
        );
    }
}
