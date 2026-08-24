// Tests assert on known-good values, so panicking there is intentional.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use anyhow::Context;
use dotenvy::dotenv;

mod application;
mod config;
mod domain;
mod infrastructure;
mod log;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // A missing .env file is fine; the environment may already have the settings.
    dotenv().ok();

    let config = config::Config::from_env().context("failed to read configuration")?;
    let _guard = log::init(&config.log_level, config.log_pretty);

    infrastructure::run(config).await
}
