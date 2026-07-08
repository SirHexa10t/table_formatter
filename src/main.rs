use std::process::ExitCode;

fn main() -> ExitCode {
    if let Err(err) = table_formatter::run() {
        eprintln!("table_formatter: {err}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
