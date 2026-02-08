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

    /// Refresh the IP-to-container mapping by querying Docker
    pub async fn refresh(&mut self, docker: &Docker, network_name: &str) -> Result<()> {
        let network = docker.inspect_network::<String>(network_name, None).await?;

        self.mapping.clear();

        if let Some(containers) = network.containers {
            for (_id, info) in containers {
                if let (Some(name), Some(ipv4)) = (info.name, info.ipv4_address) {
                    // Remove CIDR suffix (e.g., "172.18.0.2/16" -> "172.18.0.2")
                    let ip = ipv4.split('/').next().unwrap_or(&ipv4).to_string();
                    // Remove leading "/" from container names
                    let clean_name = name.trim_start_matches('/').to_string();
                    self.mapping.insert(ip, clean_name);
                }
            }
        }

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
