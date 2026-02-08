use std::time::Duration;

use bollard::Docker;
use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::app::Action;
use crate::docker::resolver::ContainerResolver;

pub async fn spawn_resolver_task(
    docker: Docker,
    network_name: String,
    action_tx: mpsc::Sender<Action>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut resolver = ContainerResolver::new();
        let mut interval = tokio::time::interval(Duration::from_secs(5));

        loop {
            interval.tick().await;

            if action_tx.is_closed() {
                break;
            }

            if let Err(e) = resolver.refresh(&docker, &network_name).await {
                warn!("Failed to refresh container mappings: {e}");
                continue;
            }

            for (ip, name) in resolver.mappings() {
                let action = Action::ContainerUpdate {
                    ip: ip.clone(),
                    name: name.clone(),
                };
                if action_tx.send(action).await.is_err() {
                    return;
                }
            }

            debug!(
                count = resolver.mappings().len(),
                "Refreshed container mappings"
            );
        }
    })
}
