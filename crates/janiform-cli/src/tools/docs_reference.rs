use std::io::Write;

use clap_markdown::MarkdownOptions;
use clap_markdown::help_markdown_command_custom;

fn main() -> std::io::Result<()> {
    let command = janiform_cli::cli_command();
    let options = MarkdownOptions::new().title("CLI Reference".to_string());
    let markdown = help_markdown_command_custom(&command, &options);

    let mut stdout = std::io::stdout().lock();
    stdout.write_all(markdown.as_bytes())?;

    if !markdown.ends_with('\n') {
        stdout.write_all(b"\n")?;
    }

    Ok(())
}
