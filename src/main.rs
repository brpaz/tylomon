use std::collections::VecDeque;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use anyhow::{Result, anyhow};
use bollard::Docker;
use clap::Parser;
use tokio::sync::mpsc;
use tracing::{error, info};

use tylomon::app::{Action, App};
use tylomon::capture::pipeline::{Recorder, spawn_replay_task};
use tylomon::cli::Cli;
use tylomon::docker::compose::{ComposeEnv, find_compose_project};
use tylomon::docker::discovery::{
    NetworkInfo, Target, discover_network_by_name, discover_project_networks,
    fill_container_counts, list_bridge_networks,
};
use tylomon::docker::task::spawn_resolver_task;
use tylomon::docker::watch::NetworkWatcher;
use tylomon::export::{Names, jsonl_line, write_har};
use tylomon::http::types::HttpExchange;
use tylomon::state::{load_last_choice, save_last_choice};
use tylomon::tui;
use tylomon::tui::event::spawn_input_task;
use tylomon::tui::picker::{PickerItem, pick};
use tylomon::tui::views;

fn install_panic_hook() {
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = tui::restore();
        original_hook(panic_info);
    }));
}

fn setup_tracing(log_file: &str) -> Result<tracing_appender::non_blocking::WorkerGuard> {
    let file_appender = tracing_appender::rolling::never(".", log_file);
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("tylomon=info")),
        )
        .with_ansi(false)
        .init();

    Ok(guard)
}

/// A row in the interactive target picker.
enum Candidate {
    Project {
        name: String,
        networks: Vec<NetworkInfo>,
    },
    Network(NetworkInfo),
}

impl Candidate {
    fn key(&self) -> String {
        match self {
            Candidate::Project { name, .. } => format!("compose:{name}"),
            Candidate::Network(net) => format!("network:{}", net.name),
        }
    }

    fn item(&self) -> PickerItem {
        let (label, detail, enabled) = match self {
            Candidate::Project { name, networks } => {
                let containers: usize = networks.iter().map(|n| n.containers).sum();
                let detail = match (networks.len(), containers) {
                    (0, _) => "not running".to_string(),
                    (_, 0) => "no running containers".to_string(),
                    (n, _) => format!("{n} network(s)"),
                };
                (format!("[compose] {name}"), detail, containers > 0)
            }
            Candidate::Network(net) => (
                format!("[bridge]  {}", net.name),
                format!("{}, {} container(s)", net.bridge_interface, net.containers),
                true,
            ),
        };
        PickerItem {
            key: self.key(),
            label,
            detail,
            enabled,
        }
    }
}

/// Let the user choose a compose project (from the current directory) or any bridge network
/// that has containers attached. A project selects all of its networks.
/// Returns `None` when the user cancels.
async fn pick_networks(docker: &Docker) -> Result<Option<(Target, Vec<NetworkInfo>)>> {
    let mut candidates = Vec::new();

    let cwd = std::env::current_dir()?;
    if let Some(project) = find_compose_project(&cwd, &ComposeEnv::from_process()) {
        let networks = discover_project_networks(docker, &project.name).await?;
        candidates.push(Candidate::Project {
            name: project.name,
            networks: fill_container_counts(docker, networks).await,
        });
    }
    let bridges = fill_container_counts(docker, list_bridge_networks(docker).await?).await;
    candidates.extend(
        bridges
            .into_iter()
            .filter(|net| net.containers > 0)
            .map(Candidate::Network),
    );

    if candidates.is_empty() {
        return Err(anyhow!(
            "No compose project in the current directory and no bridge networks with containers"
        ));
    }

    let items = candidates.iter().map(Candidate::item).collect();
    let last = load_last_choice();
    let Some(index) = pick("Select a target to monitor", items, last.as_deref())? else {
        return Ok(None);
    };

    let chosen = candidates.swap_remove(index);
    save_last_choice(&chosen.key());
    Ok(Some(match chosen {
        Candidate::Network(net) => (Target::Network(net.name.clone()), vec![net]),
        Candidate::Project { name, networks } => (Target::Project(name), networks),
    }))
}

async fn discover_networks(
    cli: &Cli,
    docker: &Docker,
) -> Result<Option<(Target, Vec<NetworkInfo>)>> {
    if let Some(ref network_name) = cli.network {
        let network = discover_network_by_name(docker, network_name).await?;
        return Ok(Some((Target::Network(network.name.clone()), vec![network])));
    }

    if let Some(ref project) = cli.project {
        let networks = discover_project_networks(docker, project).await?;
        if networks.is_empty() {
            return Err(anyhow!("No networks found for compose project '{project}'"));
        }
        return Ok(Some((Target::Project(project.clone()), networks)));
    }

    if cli.no_tui {
        return Err(anyhow!(
            "--no-tui needs a target: pass --project <compose-project> or --network <docker-network>"
        ));
    }

    pick_networks(docker).await
}

fn join_by(networks: &[NetworkInfo], field: impl Fn(&NetworkInfo) -> &str) -> String {
    networks.iter().map(field).collect::<Vec<_>>().join(", ")
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let _guard = setup_tracing(&cli.log_file)?;
    install_panic_hook();

    let (action_tx, mut action_rx) = mpsc::channel::<Action>(256);
    let mut app = App::new(cli.max_exchanges);
    let mut replay_handle = None;
    let mut watcher = None;

    if let Some(ref path) = cli.read {
        replay_handle = Some(
            spawn_replay_task(path, action_tx.clone())
                .map_err(|e| anyhow!("Failed to read '{}': {e}", path.display()))?,
        );
        app.network_label = "replay".to_string();
        app.bridge_label = path.display().to_string();
        info!(file = %path.display(), "Replaying capture file");
    } else {
        let docker = Docker::connect_with_local_defaults()?;

        let Some((target, networks)) = discover_networks(&cli, &docker).await? else {
            return Ok(());
        };
        for network in &networks {
            info!(
                network = %network.name,
                bridge = %network.bridge_interface,
                "Discovered network"
            );
        }

        let recorder = cli
            .write
            .as_deref()
            .map(Recorder::create)
            .transpose()
            .map_err(|e| anyhow!("Failed to create recording file: {e}"))?;

        app.network_label = join_by(&networks, |n| &n.name);
        app.bridge_label = join_by(&networks, |n| &n.bridge_interface);

        spawn_resolver_task(docker.clone(), target.clone(), action_tx.clone()).await;
        watcher = Some(NetworkWatcher::start(
            docker,
            target,
            networks,
            action_tx.clone(),
            recorder,
        ));
    }

    let _input_handle = (!cli.no_tui).then(|| spawn_input_task(action_tx.clone()));

    drop(action_tx);

    if cli.no_tui {
        let kept = stream_jsonl(&mut action_rx, cli.har.is_some(), cli.max_exchanges).await;
        finish(action_rx, watcher, replay_handle).await;
        let kept = kept?;
        return save_har(cli.har.as_deref(), kept.iter());
    }

    let mut terminal = tui::init()?;
    let mut render_interval = tokio::time::interval(Duration::from_millis(33));

    loop {
        tokio::select! {
            action = action_rx.recv() => {
                match action {
                    Some(action) => app.update(action),
                    None => break,
                }
            }
            _ = render_interval.tick() => {
                app.tick();
                terminal.draw(|frame| views::render(frame, &mut app))?;
            }
        }

        if app.should_quit {
            break;
        }
    }

    tui::restore()?;

    finish(action_rx, watcher, replay_handle).await;
    save_har(cli.har.as_deref(), app.exchanges.iter())
}

/// Print each completed exchange to stdout as one JSON object per line until the capture
/// ends, Ctrl-C is pressed or stdout is closed. Returns the exchanges to export, newest
/// `max_kept` only, when `keep` is set. A capture failure is returned as an error.
async fn stream_jsonl(
    action_rx: &mut mpsc::Receiver<Action>,
    keep: bool,
    max_kept: usize,
) -> Result<VecDeque<HttpExchange>> {
    let mut names = Names::new();
    let mut kept = VecDeque::new();
    let mut stdout = std::io::stdout().lock();
    // Registered once: a signal arriving while the loop body runs must not be missed.
    let ctrl_c = tokio::signal::ctrl_c();
    tokio::pin!(ctrl_c);

    loop {
        let action = tokio::select! {
            action = action_rx.recv() => action,
            _ = &mut ctrl_c => None,
        };
        match action {
            Some(Action::ContainerUpdate { ip, name }) => {
                names.insert(ip, name);
            }
            Some(Action::NewExchange(exchange)) => {
                let written = jsonl_line(&exchange, &names)
                    .map_err(std::io::Error::other)
                    .and_then(|line| writeln!(stdout, "{line}").and_then(|()| stdout.flush()));
                if let Err(e) = written {
                    // A closed pipe (e.g. `| head`) is a normal way to stop.
                    if e.kind() != std::io::ErrorKind::BrokenPipe {
                        error!("Failed to write exchange: {e}");
                    }
                    break;
                }
                if keep {
                    if kept.len() >= max_kept {
                        kept.pop_front();
                    }
                    kept.push_back(*exchange);
                }
            }
            Some(Action::CaptureError { message, .. }) => return Err(anyhow!(message)),
            Some(_) => {}
            None => break,
        }
    }

    Ok(kept)
}

fn save_har<'a>(
    path: Option<&Path>,
    exchanges: impl ExactSizeIterator<Item = &'a HttpExchange>,
) -> Result<()> {
    let Some(path) = path else {
        return Ok(());
    };
    let count = exchanges.len();
    write_har(path, exchanges)
        .map_err(|e| anyhow!("Failed to write HAR file '{}': {e}", path.display()))?;
    info!(file = %path.display(), exchanges = count, "Wrote HAR file");
    Ok(())
}

/// Stop all capture threads and wait for them, which closes any recording handle and so
/// flushes the pcap file.
async fn finish(
    action_rx: mpsc::Receiver<Action>,
    watcher: Option<NetworkWatcher>,
    replay_handle: Option<std::thread::JoinHandle<()>>,
) {
    drop(action_rx);
    if let Some(watcher) = watcher {
        watcher.shutdown().await;
    }
    if let Some(handle) = replay_handle {
        let _ = handle.join();
    }
    info!("Shutting down");
}
