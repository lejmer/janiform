use std::path::PathBuf;

use crate::context::CommandContext;

pub(crate) fn default_run_output_directory(context: &CommandContext, run_id: &str) -> PathBuf {
    context
        .paths()
        .process_cwd()
        .join(".janiform")
        .join("runs")
        .join(run_id)
}
