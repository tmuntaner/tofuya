use crate::domain::tofu::ports::{CLIPort, CleanParams, InitGitlabParams, TofuCliError};
use serde::Deserialize;
use std::process::{Command, Stdio};

#[derive(Deserialize, Clone, Default)]
pub struct CLI {}

impl CLI {
    pub fn new() -> Self {
        Self {}
    }
}

impl CLIPort for CLI {
    fn clean(&self, params: CleanParams) -> Result<(), TofuCliError> {
        let mut child = Command::new("rm")
            .current_dir(params.tf_root)
            .arg("-rf")
            .arg(".terraform")
            .spawn()?;

        child.wait()?;

        Ok(())
    }

    fn init_gitlab(&self, params: InitGitlabParams) -> Result<(), TofuCliError> {
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
