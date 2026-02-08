use clap::Parser;

/// Monitor HTTP traffic between Docker containers
#[derive(Parser, Debug)]
#[command(name = "tylomon", version, about)]
pub struct Cli {
    /// Docker compose project name to monitor
    #[arg(short, long)]
    pub project: Option<String>,

    /// Docker network name to capture on (overrides project-based discovery)
    #[arg(short, long)]
    pub network: Option<String>,

    /// Log file path for debug logging
    #[arg(long, default_value = "tylomon.log")]
    pub log_file: String,

    /// Maximum number of exchanges to keep in memory
    #[arg(long, default_value_t = 1000)]
    pub max_exchanges: usize,
}
