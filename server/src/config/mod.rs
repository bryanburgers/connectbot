use std::default::Default;
use std::path::Path;
use std::fs::File;
use std::io::Read;
use std::sync::Arc;
use toml;

pub type SharedConfig = Arc<ApplicationConfig>;

#[derive(Serialize, Deserialize, Debug)]
pub struct ApplicationConfig {
    // TODO: Do we want these somewhere else?
    pub address: String,
    pub control_address: String,
    pub tls: Tls,
    pub ssh: Ssh,
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

#[derive(Serialize, Deserialize, Debug)]
pub struct Tls {
    pub certificate: String,
    pub key: String,
}

impl Default for Tls {
    fn default() -> Self {
        Tls {
            certificate: "/etc/connectbot/server.crt".to_string(),
            key: "/etc/connectbot/server.key".to_string(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ClientAuthentication {
    pub required: bool,
    pub ca: String,
}

impl Default for ClientAuthentication {
    fn default() -> Self {
        ClientAuthentication {
            required: false,
            ca: "/etc/connectbot/ca.crt".to_string(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Ssh {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub user: Option<String>,
    pub private_key: Option<String>,
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
        }
    }
}
