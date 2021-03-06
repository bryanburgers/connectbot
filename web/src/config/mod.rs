use std::default::Default;
use std::path::Path;
use std::fs::File;
use std::io::Read;
use toml;
use tower_web::{Serialize, Deserialize};

/// The structure that represents the configuration toml file.
#[derive(Serialize, Deserialize, Debug)]
pub struct ApplicationConfig {
    /// The address to listen on
    pub address: String,
    /// The address to connect to the server on
    pub control_address: String,
    /// Where to find the .hbs template files
    pub templates: Templates,
}

impl Default for ApplicationConfig {
    fn default() -> Self {
        ApplicationConfig {
            address: "[::]:8080".to_string(),
            control_address: "[::1]:12345".to_string(),
            templates: Default::default(),
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

/// Configuration information about web templates
#[derive(Serialize, Deserialize, Debug)]
pub struct Templates {
    /// The path where the templates are found
    pub path: String,
}

impl Default for Templates {
    fn default() -> Self {
        Templates {
            path: "web/templates".to_string(),
        }
    }
}
