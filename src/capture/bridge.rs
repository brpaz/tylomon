/// Compute the Linux bridge interface name for a Docker network ID.
///
/// Docker creates bridge interfaces named `br-<first 12 chars of network ID>`.
pub fn bridge_interface_name(network_id: &str) -> String {
    let short_id = &network_id[..12.min(network_id.len())];
    format!("br-{short_id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bridge_interface_name() {
        let network_id = "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6";
        assert_eq!(bridge_interface_name(network_id), "br-a1b2c3d4e5f6");
    }

    #[test]
    fn test_bridge_interface_short_id() {
        let network_id = "abcdef";
        assert_eq!(bridge_interface_name(network_id), "br-abcdef");
    }
}
