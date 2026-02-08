use std::time::Duration;

use bollard::Docker;
use clap::Parser;
use color_eyre::eyre::{Result, eyre};
use tokio::sync::mpsc;
use tracing::info;

use tylomon::app::{Action, App};
use tylomon::capture::pipeline::spawn_capture_task;
use tylomon::cli::Cli;
use tylomon::docker::discovery::{
    NetworkInfo, discover_network_by_name, discover_project_networks,
};
use tylomon::docker::task::spawn_resolver_task;
use tylomon::tui;
use tylomon::tui::event::spawn_input_task;
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

async fn discover_network(cli: &Cli) -> Result<NetworkInfo> {
    let docker = Docker::connect_with_local_defaults()?;

    if let Some(ref network_name) = cli.network {
        return discover_network_by_name(&docker, network_name).await;
    }

    if let Some(ref project) = cli.project {
        let networks = discover_project_networks(&docker, project).await?;
        return networks
            .into_iter()
            .next()
            .ok_or_else(|| eyre!("No networks found for compose project '{project}'"));
    }

    Err(eyre!(
        "Specify either --project <compose-project> or --network <docker-network>"
    ))
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let cli = Cli::parse();

    let _guard = setup_tracing(&cli.log_file)?;
    install_panic_hook();

    let network = discover_network(&cli).await?;
    info!(
        network = %network.name,
        bridge = %network.bridge_interface,
        "Discovered network"
    );

    let docker = Docker::connect_with_local_defaults()?;

    let mut app = App::new(cli.max_exchanges);
    app.network_label = network.name.clone();
    app.bridge_label = network.bridge_interface.clone();

    let (action_tx, mut action_rx) = mpsc::channel::<Action>(256);

    let _input_handle = spawn_input_task(action_tx.clone());
    let _capture_handle = spawn_capture_task(network.bridge_interface.clone(), action_tx.clone());
    let _resolver_handle =
        spawn_resolver_task(docker, network.name.clone(), action_tx.clone()).await;

    drop(action_tx);

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
    info!("Shutting down");

    Ok(())
}
