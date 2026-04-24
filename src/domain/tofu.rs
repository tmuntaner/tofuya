use crate::core::config::{Config, StateHostType};
use crate::outbound::project_config::{ProjectConfig, ProjectGetTargetError};
use crate::outbound::tfstate::{TFStateAdapter, TFStateParseError};
use crate::outbound::tofu_cli::{CleanParams, InitGitlabParams, TofuCli, TofuCliError};
use async_trait::async_trait;
use mockall::automock;
use serde::Deserialize;
use std::path::PathBuf;
use std::process::Command;
use thiserror::Error;
use url::Url;

#[async_trait]
#[automock]
pub trait TofuPort: Send + Sync {
    async fn init(&self, params: InitParams) -> Result<(), ServiceInitError>;
    async fn list(&self) -> Result<Vec<Group>, ServiceListError>;
    async fn clean(&self) -> Result<(), ServiceCleanError>;
    async fn status(&self) -> Result<Vec<GroupStatus>, ServiceStatusError>;
}

pub struct Service {
    base_config: Config,
    project_config: ProjectConfig,
    tofu_cli: TofuCli,
    tf_state: TFStateAdapter,
}

impl Service {
    pub fn new(
        base_config: Config,
        project_config: ProjectConfig,
        tofu_cli: TofuCli,
        tf_state: TFStateAdapter,
    ) -> Self {
        Self {
            base_config,
            project_config,
            tofu_cli,
            tf_state,
        }
    }
}

#[derive(Deserialize, Clone, Default, Debug, PartialEq)]
pub enum StateType {
    #[default]
    #[serde(rename = "opentofu")]
    OpenTofu,
    #[serde(rename = "terraform")]
    Terraform,
}

impl StateType {
    pub fn binary_name(&self) -> String {
        match &self {
            StateType::OpenTofu => String::from("tofu"),
            StateType::Terraform => String::from("terraform"),
        }
    }
}

pub struct InitParams {
    pub group: String,
    pub state: String,
}

#[async_trait]
impl TofuPort for Service {
    async fn init(&self, params: InitParams) -> Result<(), ServiceInitError> {
        let target = self
            .project_config
            .get_target(params.group, params.state)?
            .ok_or(ServiceInitError::AddressNotFound)?;

        let state_host = self
            .base_config
            .get_state_host(target.url.clone())
            .ok_or(ServiceInitError::HostNotFound)?;

        match state_host._type {
            StateHostType::Gitlab => {
                let username = state_host.gitlab_username.unwrap_or_default();
                let access_token = state_host.gitlab_access_token.unwrap_or_default();

                self.tofu_cli.clean(CleanParams {
                    tf_root: target.dir.clone(),
                })?;

                self.tofu_cli.init_gitlab(InitGitlabParams {
                    state_type: target.state_type,
                    tf_root: target.dir,
                    url: target.url,
                    username,
                    access_token,
                })?;
            }
        }

        Ok(())
    }

    async fn list(&self) -> Result<Vec<Group>, ServiceListError> {
        let groups = self.project_config.list();

        Ok(groups)
    }

    async fn clean(&self) -> Result<(), ServiceCleanError> {
        let groups = self.project_config.list();

        for group in groups {
            Command::new("rm")
                .current_dir(group.dir.clone())
                .arg("-rf")
                .arg(".terraform")
                .spawn()?;
        }

        Ok(())
    }

    async fn status(&self) -> Result<Vec<GroupStatus>, ServiceStatusError> {
        let mut statuses = vec![];

        for group in self.project_config.list() {
            let tf_state = self.tf_state.parse(group.dir.clone())?;
            match tf_state {
                None => {
                    statuses.push(GroupStatus {
                        name: group.name.clone(),
                        state: None,
                        address: None,
                    });
                }
                Some(tf_state) => {
                    let address = tf_state.backend.config.address;
                    let state = self
                        .project_config
                        .state_from_address(group.name.clone(), address.clone());

                    statuses.push(GroupStatus {
                        name: group.name.clone(),
                        address: Some(address),
                        state,
                    });
                }
            }
        }

        Ok(statuses)
    }
}

#[derive(Deserialize, Clone)]
pub struct GroupStatus {
    pub name: String,
    pub state: Option<String>,
    pub address: Option<String>,
}

#[derive(Deserialize, Clone)]
pub struct StateTarget {
    pub url: Url,
    pub state_type: StateType,
    pub dir: PathBuf,
}

#[derive(Deserialize, Clone)]
pub struct GroupState {
    pub name: String,
}

#[derive(Deserialize, Clone)]
pub struct Group {
    pub name: String,
    pub states: Vec<GroupState>,
    pub dir: PathBuf,
}

#[derive(Debug, Error)]
pub enum ServiceInitError {
    #[error("address not found")]
    AddressNotFound,
    #[error("host not found")]
    HostNotFound,
    #[error(transparent)]
    TofuCLIError(#[from] TofuCliError),
    #[error(transparent)]
    ProjectConfigError(#[from] ProjectGetTargetError),
}

#[derive(Debug, Error)]
pub enum ServiceListError {}

#[derive(Debug, Error)]
pub enum ServiceCleanError {
    #[error(transparent)]
    CommandError(#[from] std::io::Error),
}

#[derive(Debug, Error)]
pub enum ServiceStatusError {
    #[error(transparent)]
    TFStateParseError(#[from] TFStateParseError),
}
