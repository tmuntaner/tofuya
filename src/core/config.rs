use serde::Deserialize;
use std::fs;
use std::path::PathBuf;
use thiserror::Error;
use url::{Host, Url};

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error(transparent)]
    FSError(#[from] std::io::Error),
    #[error(transparent)]
    SerdeError(#[from] toml::de::Error),
}

#[derive(Deserialize, Clone, Default, Debug, PartialEq)]
pub enum StateHostType {
    #[default]
    #[serde(rename = "gitlab")]
    Gitlab,
}

#[derive(Deserialize, Clone, Default)]
pub struct StateHost {
    #[serde(rename = "type")]
    pub _type: StateHostType,
    pub host: String,
    pub gitlab_username: Option<String>,
    pub gitlab_access_token: Option<String>,
}

#[derive(Deserialize, Clone, Default)]
pub struct Config {
    pub hosts: Vec<StateHost>,
}

impl Config {
    pub fn new(config_dir: Option<PathBuf>, path: Option<String>) -> Result<Self, ConfigError> {
        let config_file = path
            .map(|path| PathBuf::from(path.as_str()))
            .unwrap_or_else(|| {
                config_dir
                    .map(|config_dir| config_dir.join("tofuya").join("config.toml"))
                    .unwrap_or_default()
                    .as_path()
                    .to_owned()
            });

        if !config_file.exists() {
            return Ok(Self::default());
        }

        let config_text = fs::read_to_string(config_file)?;
        let config: Config = toml::from_str(config_text.as_str())?;

        Ok(config)
    }

    pub fn get_state_host(&self, req: Url) -> Option<StateHost> {
        for host in &self.hosts {
            let domain = Host::Domain(host.host.as_str());

            if req.host() == Some(domain) {
                return Some(host.clone());
            }
        }

        None
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn test_config() {
        let config_dir = std::env::current_dir()
            .unwrap()
            .join("testdata")
            .join("config");

        let config = Config::new(Some(config_dir), None).unwrap();
        assert_eq!(config.hosts.len(), 1);
        assert_eq!(config.hosts[0]._type, StateHostType::Gitlab);
        assert_eq!(config.hosts[0].host, "gitlab.home.arpa");
        assert_eq!(config.hosts[0].gitlab_username, Some(String::from("foo")));
        assert_eq!(
            config.hosts[0].gitlab_access_token,
            Some(String::from("bar"))
        );
    }

    #[test]
    fn test_config_does_not_exist() {
        let config_dir = std::env::current_dir()
            .unwrap()
            .join("testdata")
            .join("non_existing_config");

        let config = Config::new(Some(config_dir), None).unwrap();
        assert_eq!(config.hosts.len(), 0);
    }

    #[test]
    fn test_get_state_host() {
        let config_dir = std::env::current_dir()
            .unwrap()
            .join("testdata")
            .join("config");

        let config = Config::new(Some(config_dir), None).unwrap();
        let host = config.get_state_host(Url::parse("https://gitlab.home.arpa").unwrap());
        assert_eq!(true, host.is_some());
    }

    #[test]
    fn test_get_state_host_missing() {
        let config_dir = std::env::current_dir()
            .unwrap()
            .join("testdata")
            .join("config");

        let config = Config::new(Some(config_dir), None).unwrap();
        let host = config.get_state_host(Url::parse("https://gitlab-404.home.arpa").unwrap());
        assert_eq!(true, host.is_none());
    }
}
