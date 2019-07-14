use std::default::Default;
use std::path::Path;
use std::fs::File;
use std::io::Read;
use std::sync::Arc;
use toml;

pub type SharedConfig = Arc<ApplicationConfig>;

/// The structure that represents the configuration toml file.
#[derive(Serialize, Deserialize, Debug)]
pub struct ApplicationConfig {
    /// The address/port to listen on for client connections.
    pub address: String,
    /// The address/port to listen on for control (connectbot-ctrl, connectbot-web) connections.
    pub control_address: String,
    /// TLS information (see below)
    pub tls: Tls,
    /// SSH information (see below)
    pub ssh: Ssh,
    /// Client authentication information (see below)
    pub client_authentication: Option<ClientAuthentication>,
}

impl Default for ApplicationConfig {
    fn default() -> Self {
        ApplicationConfig {
            tls: Default::default(),
            ssh: Default::default(),
            client_authentication: Some(Default::default()),
            address: "[::]:4004".to_string(),
            control_address: "[::1]:12345".to_string(),
        }
    }
}

impl ApplicationConfig {
    pub fn from_file(config_file: &Path) -> Result<ApplicationConfig, String> {
        let mut data = String::new();

        let mut file = File::open(config_file)
            .map_err(|err| format!("Failed to open {:?}: {}", config_file, err))?;

        file.read_to_string(&mut data)
            .map_err(|err| format!("Failed to read {:?}: {}", config_file, err))?;

        let config = toml::from_str(&data)
            .map_err(|err| format!("Failed to parse {:?}: {}", config_file, err))?;

        Ok(config)
    }
}

/// TLS configuration
#[derive(Serialize, Deserialize, Debug)]
pub struct Tls {
    /// The path to the TLS public certificate
    pub certificate: String,
    /// The path to the TLS private key
    pub key: String,
}

impl Default for Tls {
    fn default() -> Self {
        Tls {
            certificate: "./server.crt".to_string(),
            key: "./server-private.pem".to_string(),
        }
    }
}

/// Client authentication information
#[derive(Serialize, Deserialize, Debug)]
pub struct ClientAuthentication {
    /// Whether the client needs to send a client certificate
    pub required: bool,
    /// The CA to validate the client certificate against.
    pub ca: String,
}

impl Default for ClientAuthentication {
    fn default() -> Self {
        ClientAuthentication {
            required: false,
            ca: "./ca.crt".to_string(),
        }
    }
}

/// Information about how to establish SSH connections
#[derive(Serialize, Deserialize, Debug)]
pub struct Ssh {
    /// The DNS name of the host that the server is on.
    pub host: Option<String>,
    /// The SSH port. Probably 22.
    pub port: Option<u16>,
    /// The user to use when clients establish a connection.
    pub user: Option<String>,
    /// The path to the SSH private key to send clients.
    pub private_key: Option<String>,
    /// The start of the port range to use for non-web forwards.
    pub port_start: u16,
    /// The end of the port range (inclusive) to use for non-web forwards.
    pub port_end: u16,
    /// The start of the port range to use for web forwards.
    pub web_port_start: u16,
    /// The end of the port range (inclusive) to use for web forwards.
    pub web_port_end: u16,
    /// The contents of the SSH private key.
    #[serde(skip_serializing)]
    #[serde(skip_deserializing)]
    pub private_key_data: Option<String>,
}

impl Default for Ssh {
    fn default() -> Self {
        Ssh {
            host: Some("host.tld".into()),
            port: Some(22),
            user: Some("reversessh".into()),
            private_key: Some("/home/reversessh/.ssh/id_rsa".into()),
            private_key_data: None,
            port_start: 10000,
            port_end: 10999,
            web_port_start: 7000,
            web_port_end: 7016,
        }
    }
}
