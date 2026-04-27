use crate::domain::tofu::ports::{TFState, TFStateParseError, TFStatePort};
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

#[derive(Deserialize, Clone, Default)]
pub struct TFStateAdapter {}

impl TFStateAdapter {
    pub fn new() -> Self {
        Self {}
    }
}

impl TFStatePort for TFStateAdapter {
    fn parse(&self, tf_root: PathBuf) -> Result<Option<TFState>, TFStateParseError> {
        let path = tf_root.join(".terraform").join("terraform.tfstate");

        if !path.exists() {
            return Ok(None);
        }

        let config_text = fs::read_to_string(path)?;
        let config: TFState = serde_json::from_str(config_text.as_str())?;

        Ok(Some(config))
    }
}
