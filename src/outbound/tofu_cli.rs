use crate::domain::tofu::StateType;
use serde::Deserialize;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use thiserror::Error;
use url::Url;

#[derive(Deserialize, Clone, Default)]
pub struct TofuCli {}

impl TofuCli {
    pub fn new() -> Self {
        Self {}
    }

    pub fn clean(&self, params: CleanParams) -> Result<(), TofuCliError> {
        let mut child = Command::new("rm")
            .current_dir(params.tf_root)
            .arg("-rf")
            .arg(".terraform")
            .spawn()?;

        child.wait()?;

        Ok(())
    }

    pub fn init_gitlab(&self, params: InitGitlabParams) -> Result<(), TofuCliError> {
        let mut child = Command::new(params.state_type.binary_name())
            .current_dir(params.tf_root)
            .arg("init")
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .arg(format!("-backend-config=address={}", params.url))
            .arg(format!("-backend-config=lock_address={}/lock", params.url))
            .arg(format!(
                "-backend-config=unlock_address={}/lock",
                params.url
            ))
            .arg(format!("-backend-config=username={}", params.username))
            .arg(format!("-backend-config=password={}", params.access_token))
            .arg("-backend-config=lock_method=POST")
            .arg("-backend-config=unlock_method=DELETE")
            .arg("-backend-config=retry_wait_min=5")
            .spawn()?;

        child.wait()?;

        Ok(())
    }
}

pub struct CleanParams {
    pub tf_root: PathBuf,
}

pub struct InitGitlabParams {
    pub state_type: StateType,
    pub tf_root: PathBuf,
    pub url: Url,
    pub username: String,
    pub access_token: String,
}

#[derive(Debug, Error)]
pub enum TofuCliError {
    #[error(transparent)]
    CommandError(#[from] std::io::Error),
}
