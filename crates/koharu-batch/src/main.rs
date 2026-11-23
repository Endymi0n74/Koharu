//! `koharu-batch`: one chapter, end to end, from a terminal.

mod cli;
mod run;

use anyhow::Result;
use clap::Parser;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new("warn,koharu_batch=info,koharu_pipeline=info")
            }),
        )
        .with_writer(std::io::stderr)
        .init();
    let arguments = cli::Arguments::parse();
    run::run(&arguments).await
}
