use serde::Deserialize;
use std::fs;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Deserialize, Clone, Default)]
pub struct TFStateAdapter {}

impl TFStateAdapter {
    pub fn new() -> Self {
        Self {}
    }

    pub fn parse(&self, tf_root: PathBuf) -> Result<Option<TFState>, TFStateParseError> {
        let path = tf_root.join(".terraform").join("terraform.tfstate");

        if !path.exists() {
            return Ok(None);
        }

        let config_text = fs::read_to_string(path)?;
        let config: TFState = serde_json::from_str(config_text.as_str())?;

        Ok(Some(config))
    }
}

#[derive(Deserialize, Clone, Default)]
pub struct TFStateBackendConfig {
    pub address: String,
}

#[derive(Deserialize, Clone, Default)]
pub struct TFStateBackend {
    pub config: TFStateBackendConfig,
}
#[derive(Deserialize, Clone, Default)]
pub struct TFState {
    pub backend: TFStateBackend,
}

#[derive(Error, Debug)]
pub enum TFStateParseError {
    #[error(transparent)]
    FSError(#[from] std::io::Error),
    #[error(transparent)]
    SerdeError(#[from] serde_json::Error),
}
