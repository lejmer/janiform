use std::ffi::OsStr;
use std::ffi::OsString;

pub(crate) fn rewrite_beginner_fit_arguments(arguments: Vec<OsString>) -> Vec<OsString> {
    let Some(fit_argument_position) = root_fit_argument_position(&arguments) else {
        return arguments;
    };

    if arguments.len() <= fit_argument_position + 1 {
        return arguments;
    }

    match arguments
        .get(fit_argument_position + 1)
        .map(OsString::as_os_str)
    {
        Some(command) if command == OsStr::new("inspect") => {
            rewrite_fit_inspect_arguments(arguments, fit_argument_position)
        }
        Some(command)
            if command == OsStr::new("plan")
                || command == OsStr::new("staged")
                || command.as_encoded_bytes().starts_with(b"-") =>
        {
            arguments
        }
        Some(_) => rewrite_fit_arguments(arguments, fit_argument_position),
        None => arguments,
    }
}

fn root_fit_argument_position(arguments: &[OsString]) -> Option<usize> {
    let mut argument_position = 1;
    while argument_position < arguments.len() {
        let argument = arguments.get(argument_position)?.as_os_str();
        if argument == OsStr::new("fit") {
            return Some(argument_position);
        }

        if !argument.as_encoded_bytes().starts_with(b"-") {
            return None;
        }

        if global_option_requires_value(argument) {
            argument_position += 2;
        } else {
            argument_position += 1;
        }
    }

    None
}

fn global_option_requires_value(argument: &OsStr) -> bool {
    if argument.as_encoded_bytes().contains(&b'=') {
        return false;
    }

    argument == OsStr::new("--config")
        || argument == OsStr::new("--color")
        || argument == OsStr::new("--events")
}

fn rewrite_fit_arguments(arguments: Vec<OsString>, fit_argument_position: usize) -> Vec<OsString> {
    let mut rewritten_arguments = Vec::with_capacity(arguments.len() + 1);
    let mut argument_iterator = arguments.into_iter();

    for _ in 0..=fit_argument_position {
        if let Some(argument) = argument_iterator.next() {
            rewritten_arguments.push(argument);
        }
    }

    if let Some(tick_data_path) = argument_iterator.next() {
        rewritten_arguments.push(OsString::from("--tick-data-path"));
        rewritten_arguments.push(tick_data_path);
    }

    rewritten_arguments.extend(argument_iterator);
    rewritten_arguments
}

fn rewrite_fit_inspect_arguments(
    arguments: Vec<OsString>,
    fit_argument_position: usize,
) -> Vec<OsString> {
    if arguments.len() <= fit_argument_position + 2 {
        return arguments;
    }

    match arguments
        .get(fit_argument_position + 2)
        .map(OsString::as_os_str)
    {
        Some(command) if command.as_encoded_bytes().starts_with(b"-") => arguments,
        Some(_) => {
            let mut rewritten_arguments = Vec::with_capacity(arguments.len() + 1);
            let mut argument_iterator = arguments.into_iter();

            for _ in 0..=(fit_argument_position + 1) {
                if let Some(argument) = argument_iterator.next() {
                    rewritten_arguments.push(argument);
                }
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

    #[test]
    fn rewrites_fit_path_after_root_global_options() {
        assert_eq!(
            rewrite(&[
                "janiform",
                "--json",
                "--color",
                "never",
                "fit",
                "ticks.parquet"
            ]),
            vec![
                OsString::from("janiform"),
                OsString::from("--json"),
                OsString::from("--color"),
                OsString::from("never"),
                OsString::from("fit"),
                OsString::from("--tick-data-path"),
                OsString::from("ticks.parquet"),
            ],
        );
    }
}
