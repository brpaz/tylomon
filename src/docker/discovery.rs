use anyhow::{Result, anyhow};
use bollard::Docker;

/// Information about a discovered Docker network
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkInfo {
    pub id: String,
    pub name: String,
    pub bridge_interface: String,
    /// Attached container count; zero until [`fill_container_counts`] runs.
    pub containers: usize,
}

/// Build a [`NetworkInfo`], honouring a custom bridge name (e.g. `docker0` for the default
/// network) before falling back to `br-<short id>`.
fn network_info(
    id: String,
    name: String,
    options: Option<&std::collections::HashMap<String, String>>,
) -> NetworkInfo {
    let bridge_interface = options
        .and_then(|o| o.get("com.docker.network.bridge.name"))
        .cloned()
        .unwrap_or_else(|| crate::capture::bridge::bridge_interface_name(&id));
    NetworkInfo {
        id,
        name,
        bridge_interface,
        containers: 0,
    }
}

/// Fill in the attached container count of each network, leaving it at zero when a network
/// cannot be inspected.
pub async fn fill_container_counts(
    docker: &Docker,
    networks: Vec<NetworkInfo>,
) -> Vec<NetworkInfo> {
    futures::future::join_all(networks.into_iter().map(|mut net| async move {
        if let Ok(inspected) = docker.inspect_network(&net.id, None).await {
            net.containers = inspected.containers.map_or(0, |c| c.len());
        }
        net
    }))
    .await
}

/// List every bridge-driver network known to the Docker daemon.
pub async fn list_bridge_networks(docker: &Docker) -> Result<Vec<NetworkInfo>> {
    let networks = docker
        .list_networks(None)
        .await
        .map_err(|e| anyhow!("Failed to list Docker networks: {e}"))?;

    let mut results: Vec<NetworkInfo> = networks
        .into_iter()
        .filter(|net| net.driver.as_deref() == Some("bridge"))
        .filter_map(|net| Some(network_info(net.id?, net.name?, net.options.as_ref())))
        .collect();
    results.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(results)
}

/// What the user asked to monitor, kept so it can be re-resolved after networks are recreated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Project(String),
    Network(String),
}

impl Target {
    /// Current networks matching this target. A network that does not exist (yet) resolves
    /// to an empty list.
    pub async fn resolve(&self, docker: &Docker) -> Result<Vec<NetworkInfo>> {
        match self {
            Target::Project(project) => discover_project_networks(docker, project).await,
            Target::Network(name) => {
                // The name filter is a substring match.
                let networks = list_networks_filtered(docker, "name", name).await?;
                Ok(networks.into_iter().filter(|n| &n.name == name).collect())
            }
        }
    }
}

/// Networks matching a Docker list filter such as `label=...` or `name=...`.
async fn list_networks_filtered(
    docker: &Docker,
    key: &str,
    value: &str,
) -> Result<Vec<NetworkInfo>> {
    use bollard::query_parameters::ListNetworksOptionsBuilder;
    use std::collections::HashMap;

    let filters = HashMap::from([(key, vec![value])]);
    let options = ListNetworksOptionsBuilder::default()
        .filters(&filters)
        .build();

    let networks = docker
        .list_networks(Some(options))
        .await
        .map_err(|e| anyhow!("Failed to list Docker networks: {e}"))?;

    Ok(networks
        .into_iter()
        .filter_map(|net| Some(network_info(net.id?, net.name?, net.options.as_ref())))
        .collect())
}

/// Discover Docker networks for a compose project.
///
/// Compose networks follow the naming convention: `<project>_<network>`.
/// The default network is named `<project>_default`.
pub async fn discover_project_networks(docker: &Docker, project: &str) -> Result<Vec<NetworkInfo>> {
    list_networks_filtered(
        docker,
        "label",
        &format!("com.docker.compose.project={project}"),
    )
    .await
}

/// Discover a network by name directly (when --network flag is used)
pub async fn discover_network_by_name(docker: &Docker, network_name: &str) -> Result<NetworkInfo> {
    let network = docker
        .inspect_network(network_name, None)
        .await
        .map_err(|e| anyhow!("Failed to inspect network '{network_name}': {e}"))?;

    let id = network.id.ok_or_else(|| anyhow!("Network has no ID"))?;
    let name = network.name.ok_or_else(|| anyhow!("Network has no name"))?;

    Ok(network_info(id, name, network.options.as_ref()))
}
