use janiform_event_log::default_run_event_paths;

use crate::commands::inspection::InspectRunCommand;
use crate::commands::shared::output::write_json_payload;
use crate::context::CommandContext;
use crate::context::OutputMode;
use crate::error::CliError;

use super::payload::run_inspection_payload;
use super::rendering::render_run_state;
use super::state::load_inspected_run_state;

pub(crate) fn run_inspect_run(
    context: &CommandContext,
    command: InspectRunCommand,
) -> Result<(), CliError> {
    let run_directory = context.paths().resolve_process_path(&command.run_directory);
    let event_paths = default_run_event_paths(&run_directory);

    let inspected_state = load_inspected_run_state(&run_directory)?;

    let payload = run_inspection_payload(
        run_directory.display().to_string(),
        &event_paths,
        &inspected_state,
    );

    match context.output_mode() {
        OutputMode::Json => write_json_payload(&payload),
        OutputMode::Silent => Ok(()),
        OutputMode::Terminal | OutputMode::Quiet => {
            let rendered = render_run_state(
                &payload.state,
                &run_directory,
                command.include_events || context.verbosity() > 0,
                &event_paths.event_log_path,
                payload.inspection.state_source,
                &context.stdout_renderer(),
            );
            context
                .stdout_renderer()
                .write_stdout(&rendered)
                .map_err(|source| CliError::OutputWrite { source })
        }
    }
}
