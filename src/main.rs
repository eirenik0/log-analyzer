use log_analyzer::{Commands, cli_parse, compare_logs, display_log_info, parse_log_file};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = cli_parse();

    match &cli.command {
        Commands::Compare {
            file1,
            file2,
            component,
            level,
            contains,
            diff_only,
            output,
            full,
        } => {
            let logs1 = parse_log_file(file1).unwrap();
            let logs2 = parse_log_file(file2).unwrap();

            compare_logs(
                &logs1,
                &logs2,
                component.as_deref(),
                level.as_deref(),
                contains.as_deref(),
                *diff_only,
                output.as_deref(),
                *full,
            )?;
        }
        Commands::Info { file } => {
            let logs = parse_log_file(file).unwrap();
            display_log_info(&logs);
        }
    }

    Ok(())
}
