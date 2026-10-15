use std::collections::HashMap;
use std::time::Duration;

use bollard::Docker;
use tokio::sync::{mpsc, oneshot};
use tracing::{info, warn};

use crate::app::Action;
use crate::capture::pipeline::{CaptureHandle, Recorder, spawn_capture_task};
use crate::docker::discovery::{NetworkInfo, Target};

const POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Keeps one capture per network of a target, following networks as they are removed,
/// recreated (new bridge interface) or added.
pub struct NetworkWatcher {
    cancel: oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}

impl NetworkWatcher {
    /// Start capturing on `initial` and keep them in sync with Docker.
    pub fn start(
        docker: Docker,
        target: Target,
        initial: Vec<NetworkInfo>,
        action_tx: mpsc::Sender<Action>,
        recorder: Option<Recorder>,
    ) -> Self {
        let (cancel, cancelled) = oneshot::channel();
        let task = tokio::spawn(watch(
            docker, target, initial, action_tx, recorder, cancelled,
        ));
        Self { cancel, task }
    }

    /// Stop every capture and wait for the threads to finish.
    pub async fn shutdown(self) {
        let _ = self.cancel.send(());
        let _ = self.task.await;
    }
}

/// Changes needed to bring running captures in line with the networks Docker reports.
#[derive(Debug, Default, PartialEq, Eq)]
struct Plan {
    /// Names of networks whose capture must stop.
    stop: Vec<String>,
    /// Networks to start capturing on.
    start: Vec<NetworkInfo>,
}

/// `tracked` maps network name to `(network id, capture ended on its own)`.
fn plan(tracked: &HashMap<String, (String, bool)>, current: &[NetworkInfo]) -> Plan {
    let mut plan = Plan::default();

    for (name, (id, ended)) in tracked {
        match current.iter().find(|n| &n.name == name) {
            Some(net) if &net.id == id && !ended => {}
            Some(net) => {
                plan.stop.push(name.clone());
                plan.start.push(net.clone());
            }
            None => plan.stop.push(name.clone()),
        }
    }
    plan.start.extend(
        current
            .iter()
            .filter(|n| !tracked.contains_key(&n.name))
            .cloned(),
    );

    plan.stop.sort();
    plan.start.sort_by(|a, b| a.name.cmp(&b.name));
    plan
}

struct Tracked {
    network: NetworkInfo,
    capture: CaptureHandle,
}

async fn watch(
    docker: Docker,
    target: Target,
    initial: Vec<NetworkInfo>,
    action_tx: mpsc::Sender<Action>,
    recorder: Option<Recorder>,
    mut cancelled: oneshot::Receiver<()>,
) {
    let start_capture = |network: NetworkInfo| Tracked {
        capture: spawn_capture_task(
            network.bridge_interface.clone(),
            action_tx.clone(),
            recorder.clone(),
        ),
        network,
    };

    let mut tracked: HashMap<String, Tracked> = initial
        .into_iter()
        .map(|n| (n.name.clone(), start_capture(n)))
        .collect();

    let mut interval = tokio::time::interval(POLL_INTERVAL);
    interval.tick().await;

    loop {
        tokio::select! {
            _ = interval.tick() => {}
            _ = &mut cancelled => break,
        }
        if action_tx.is_closed() {
            break;
        }

        let current = match target.resolve(&docker).await {
            Ok(current) => current,
            Err(e) => {
                warn!("Failed to refresh networks: {e}");
                continue;
            }
        };

        let view: HashMap<String, (String, bool)> = tracked
            .iter()
            .map(|(name, t)| {
                (
                    name.clone(),
                    (t.network.id.clone(), t.capture.needs_restart()),
                )
            })
            .collect();
        let changes = plan(&view, &current);
        if changes == Plan::default() {
            continue;
        }

        let mut notices = Vec::new();
        // Networks whose capture ended on its own are retried quietly.
        let mut retried = HashMap::new();
        for name in &changes.stop {
            if let Some(old) = tracked.remove(name) {
                old.capture.stop();
                if changes.start.iter().any(|n| &n.name == name) {
                    retried.insert(name.clone(), old.network.id);
                } else {
                    notices.push(format!("Network {name} removed, waiting for it to return"));
                }
            }
        }
        for network in changes.start {
            if retried.get(&network.name) != Some(&network.id) {
                notices.push(format!(
                    "Capturing on {} ({})",
                    network.name, network.bridge_interface
                ));
            }
            info!(network = %network.name, bridge = %network.bridge_interface, "Attaching capture");
            tracked.insert(network.name.clone(), start_capture(network));
        }

        let mut labels: Vec<_> = tracked
            .values()
            .map(|t| (t.network.name.clone(), t.network.bridge_interface.clone()))
            .collect();
        labels.sort();
        let sent = async {
            for notice in notices {
                action_tx.send(Action::Notice(notice)).await.ok()?;
            }
            action_tx.send(Action::Networks(labels)).await.ok()
        }
        .await;
        if sent.is_none() {
            break;
        }
    }

    for (_, t) in tracked {
        t.capture.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn net(name: &str, id: &str) -> NetworkInfo {
        NetworkInfo {
            id: id.into(),
            name: name.into(),
            bridge_interface: format!("br-{id}"),
            containers: 0,
        }
    }

    fn tracked(entries: &[(&str, &str, bool)]) -> HashMap<String, (String, bool)> {
        entries
            .iter()
            .map(|(name, id, ended)| (name.to_string(), (id.to_string(), *ended)))
            .collect()
    }

    #[test]
    fn unchanged_networks_need_no_work() {
        let plan = plan(&tracked(&[("a", "1", false)]), &[net("a", "1")]);
        assert_eq!(plan, Plan::default());
    }

    #[test]
    fn recreated_network_is_restarted_on_new_bridge() {
        let plan = plan(&tracked(&[("a", "1", false)]), &[net("a", "2")]);
        assert_eq!(plan.stop, ["a"]);
        assert_eq!(plan.start, [net("a", "2")]);
    }

    #[test]
    fn removed_network_is_stopped_without_restart() {
        let plan = plan(&tracked(&[("a", "1", false)]), &[]);
        assert_eq!(plan.stop, ["a"]);
        assert!(plan.start.is_empty());
    }

    #[test]
    fn returning_network_is_started() {
        let plan = plan(&tracked(&[]), &[net("a", "1")]);
        assert!(plan.stop.is_empty());
        assert_eq!(plan.start, [net("a", "1")]);
    }

    #[test]
    fn capture_that_ended_is_restarted_when_network_still_exists() {
        let plan = plan(&tracked(&[("a", "1", true)]), &[net("a", "1")]);
        assert_eq!(plan.stop, ["a"]);
        assert_eq!(plan.start, [net("a", "1")]);
    }

    #[test]
    fn project_gaining_a_network_starts_only_the_new_one() {
        let plan = plan(
            &tracked(&[("a", "1", false)]),
            &[net("a", "1"), net("b", "2")],
        );
        assert!(plan.stop.is_empty());
        assert_eq!(plan.start, [net("b", "2")]);
    }
}
