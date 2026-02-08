use std::fmt;

/// Application-specific error types
#[derive(Debug)]
pub enum Error {
    /// No Docker network found for the given project
    NetworkNotFound(String),
    /// Bridge interface not found for the given network
    BridgeNotFound(String),
    /// Packet capture error
    CaptureError(String),
    /// Docker API error
    DockerError(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NetworkNotFound(project) => {
                write!(f, "No Docker network found for project: {project}")
            }
            Error::BridgeNotFound(network) => {
                write!(f, "Bridge interface not found for network: {network}")
            }
            Error::CaptureError(msg) => write!(f, "Capture error: {msg}"),
            Error::DockerError(msg) => write!(f, "Docker error: {msg}"),
        }
    }
}

impl std::error::Error for Error {}
