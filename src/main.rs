use std::process;

// Handle Box<dyn Error> properly
fn display_error(err: Box<dyn std::error::Error>) -> String {
    let mut error_msg = err.to_string();
    let mut source = err.source();
    while let Some(err) = source {
        error_msg.push_str(&format!("\nCaused by: {}", err));
        source = err.source();
    }
    error_msg
}

fn main() {
    if let Err(err) = log_analyzer::run() {
        eprintln!("Error: {}", display_error(err));
        process::exit(1);
    }
}
