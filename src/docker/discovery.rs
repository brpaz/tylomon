use bollard::Docker;
use color_eyre::eyre::{Result, eyre};

/// Information about a discovered Docker network
#[derive(Debug, Clone)]
pub struct NetworkInfo {
    pub id: String,
    pub name: String,
    pub bridge_interface: String,
}

/// Discover Docker networks for a compose project.
///
/// Compose networks follow the naming convention: `<project>_<network>`.
/// The default network is named `<project>_default`.
pub async fn discover_project_networks(docker: &Docker, project: &str) -> Result<Vec<NetworkInfo>> {
    use bollard::network::ListNetworksOptions;
    use std::collections::HashMap;

    let filters: HashMap<String, Vec<String>> = HashMap::from([(
        "label".to_string(),
        vec![format!("com.docker.compose.project={project}")],
    )]);

    let networks = docker
        .list_networks(Some(ListNetworksOptions { filters }))
        .await
        .map_err(|e| eyre!("Failed to list Docker networks: {e}"))?;

    let results: Vec<NetworkInfo> = networks
        .into_iter()
        .filter_map(|net| {
            let id = net.id?;
            let name = net.name?;
            let bridge = crate::capture::bridge::bridge_interface_name(&id);
            Some(NetworkInfo {
                id,
                name,
                bridge_interface: bridge,
            })
        })
        .collect();

    Ok(results)
}

/// Discover a network by name directly (when --network flag is used)
pub async fn discover_network_by_name(docker: &Docker, network_name: &str) -> Result<NetworkInfo> {
    let network = docker
        .inspect_network::<String>(network_name, None)
        .await
        .map_err(|e| eyre!("Failed to inspect network '{network_name}': {e}"))?;

    let id = network.id.ok_or_else(|| eyre!("Network has no ID"))?;
    let name = network.name.ok_or_else(|| eyre!("Network has no name"))?;
    let bridge = crate::capture::bridge::bridge_interface_name(&id);

    Ok(NetworkInfo {
        id,
        name,
        bridge_interface: bridge,
    })
}
