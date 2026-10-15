use std::collections::HashMap;

use anyhow::Result;
use bollard::Docker;

/// Resolves container IPs to container/service names
pub struct ContainerResolver {
    /// IP address -> container/service name
    mapping: HashMap<String, String>,
}

impl Default for ContainerResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl ContainerResolver {
    pub fn new() -> Self {
        Self {
            mapping: HashMap::new(),
        }
    }

    /// Refresh the IP-to-container mapping by querying Docker for every given network.
    /// The previous mapping is kept if any network cannot be inspected.
    pub async fn refresh(&mut self, docker: &Docker, network_names: &[String]) -> Result<()> {
        let mut mapping = HashMap::new();

        for network_name in network_names {
            let network = docker.inspect_network(network_name, None).await?;

            for (_id, info) in network.containers.unwrap_or_default() {
                if let (Some(name), Some(ipv4)) = (info.name, info.ipv4_address) {
                    // Drop the CIDR suffix of the address
                    let ip = ipv4.split('/').next().unwrap_or(&ipv4).to_string();
                    // Docker prefixes container names with "/"
                    let clean_name = name.trim_start_matches('/').to_string();
                    mapping.insert(ip, clean_name);
                }
            }
        }

        self.mapping = mapping;
        Ok(())
    }

    /// Look up a container name by IP address
    pub fn resolve(&self, ip: &str) -> Option<&str> {
        self.mapping.get(ip).map(String::as_str)
    }

    /// Get all current mappings (for sending to UI)
    pub fn mappings(&self) -> &HashMap<String, String> {
        &self.mapping
    }
}
