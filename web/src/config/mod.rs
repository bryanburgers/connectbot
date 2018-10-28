use std::default::Default;
use std::path::Path;
use std::fs::File;
use std::io::Read;
use toml;

#[derive(Serialize, Deserialize, Debug)]
pub struct ApplicationConfig {
    pub address: String,
    pub control_address: String,
}

impl Default for ApplicationConfig {
    fn default() -> Self {
        ApplicationConfig {
            address: "[::]:8080".to_string(),
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
