//! `swayconnect`: the reference client (`docs/DESIGN.md` 7.2).

#![forbid(unsafe_code)]

use std::process::ExitCode;

use clap::Parser as _;
use swayconnect_cli::cli::{
    Cli, CliError, Context, DirectoryInputs, ProfileStore, client_directory, run,
};
use swayconnect_cli::client::Connector;

const CLIENT_ID: &str = concat!("swayconnect-cli/", env!("CARGO_PKG_VERSION"));

fn main() -> ExitCode {
    let cli = Cli::parse();
    match execute(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("swayconnect: {error}");
            ExitCode::from(error.exit_code())
        }
    }
}

fn execute(cli: &Cli) -> Result<(), CliError> {
    let directory = client_directory(&DirectoryInputs {
        client_dir: std::env::var_os("SWAYCONNECT_CLIENT_DIR"),
        xdg_data_home: std::env::var_os("XDG_DATA_HOME"),
        home: std::env::var_os("HOME"),
    })?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|source| CliError::Io {
            action: "start the async runtime",
            source,
        })?;
    let context = Context {
        connector: Connector::new(Some(CLIENT_ID.to_owned())),
        store: ProfileStore::new(directory),
        input: &mut std::io::stdin().lock(),
        output: &mut std::io::stdout().lock(),
    };
    runtime.block_on(run(cli, context))
}
