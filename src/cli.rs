use std::path::PathBuf;

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

    /// Record captured packets to a pcap file
    #[arg(short, long, value_name = "FILE", conflicts_with = "read")]
    pub write: Option<PathBuf>,

    /// Replay a pcap file recorded with --write instead of capturing live (needs no
    /// privileges or Docker)
    #[arg(short, long, value_name = "FILE", conflicts_with_all = ["project", "network"])]
    pub read: Option<PathBuf>,

    /// Write exchanges to stdout as JSON Lines instead of showing the TUI
    #[arg(long)]
    pub no_tui: bool,

    /// Save all captured exchanges to a HAR file on exit
    #[arg(long, value_name = "FILE")]
    pub har: Option<PathBuf>,

    /// Log file path for debug logging
    #[arg(long, default_value = "tylomon.log")]
    pub log_file: String,

    /// Maximum number of exchanges to keep in memory
    #[arg(long, default_value_t = 1000)]
    pub max_exchanges: usize,
}
