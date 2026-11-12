use elsecrew_sidecar::{run_cli, STARTUP_FAILURE_EXIT_CODE};

fn main() {
    if let Err(error) = run_cli(std::env::args_os()) {
        eprintln!("{}", error.detail());
        std::process::exit(STARTUP_FAILURE_EXIT_CODE);
    }
}
