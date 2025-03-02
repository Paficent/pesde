use crate::cli::{
    config::read_config,
    version::{
        current_version, get_latest_remote_version, get_or_download_version, update_bin_exe,
    },
};
use anyhow::Context;
use clap::Args;
use colored::Colorize;

#[derive(Debug, Args)]
pub struct SelfUpgradeCommand {
    /// Whether to use the version from the "upgrades available" message
    #[clap(long, default_value_t = false)]
    use_cached: bool,
}

impl SelfUpgradeCommand {
    pub async fn run(self, reqwest: reqwest::Client) -> anyhow::Result<()> {
        let latest_version = if self.use_cached {
            read_config()
                .await?
                .last_checked_updates
                .context("no cached version found")?
                .1
        } else {
            get_latest_remote_version(&reqwest).await?
        };

        if latest_version <= current_version() {
            println!("already up to date");
            return Ok(());
        }

        if !inquire::prompt_confirmation(format!(
            "are you sure you want to upgrade {} from {} to {}?",
            env!("CARGO_BIN_NAME").cyan(),
            current_version().to_string().yellow().bold(),
            latest_version.to_string().yellow().bold()
        ))? {
            println!("cancelled upgrade");
            return Ok(());
        }

        let path = get_or_download_version(&reqwest, &latest_version, true)
            .await?
            .unwrap();
        update_bin_exe(&path).await?;

        println!(
            "upgraded to version {}!",
            latest_version.to_string().yellow().bold()
        );

        Ok(())
    }
}
