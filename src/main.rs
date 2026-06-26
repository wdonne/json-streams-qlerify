pub mod api;
mod app;
mod common;
mod error;
mod model;

use log::error;
use std::process::exit;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Generate JSON Streams apps from a Qlerify event-model
    App(AppArgs),
    /// Generate OpenAPI specifications for JSON Streams apps from a Qlerify event-model
    Api(ApiArgs),
}

#[derive(clap::Args, Debug)]
#[command(about, long_about = None)]
struct AppArgs {
    #[arg(
        short,
        long,
        default_value = ".",
        long_help = "The directory in which the API is generated. The default is the current working directory."
    )]
    directory: std::path::PathBuf,
    #[arg(
        short,
        long,
        long_help = "The file containing the event-model in JSON format."
    )]
    file: std::path::PathBuf,
    #[arg(
        short,
        long,
        default_value_t = false,
        long_help = "Turn on mock mode, where simple reducers are generated each time."
    )]
    mock_mode: bool,
}

#[derive(clap::Args, Debug)]
#[command(about, long_about = None)]
struct ApiArgs {
    #[arg(
        short,
        long,
        default_value = ".",
        long_help = "The directory in which the API is generated. The default is the current working directory."
    )]
    directory: std::path::PathBuf,
    #[arg(
        short,
        long,
        long_help = "The file containing the event-model in JSON format."
    )]
    file: std::path::PathBuf,
    #[arg(
        short,
        long,
        long_help = "The file containing the OpenAPI template, in which the generated specification will be merged.
It may be in JSON or YAML format, which is determined through the extension of the filename."
    )]
    template: Option<std::path::PathBuf>,
}

fn main() {
    stderrlog::new()
        .module(module_path!())
        .verbosity(3)
        .init()
        .unwrap();

    if let Err(e) = match Cli::parse().command {
        Commands::App(a) => app::generate(&a),
        Commands::Api(a) => api::generate(&a),
    } {
        error!("{0}", e);
        exit(1)
    }
}
