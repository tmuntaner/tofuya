use crate::domain::tofu::models::{ConfigStateHostType, Group, GroupStatus};
use crate::domain::tofu::ports::{
    CLIPort, CleanParams, ConfigPort, InitGitlabParams, ProjectConfigPort, ProjectGetTargetError,
    TFStateParseError, TFStatePort, TofuCliError,
};
use async_trait::async_trait;
use mockall::automock;
use std::process::Command;
use std::sync::Arc;
use thiserror::Error;

#[async_trait]
#[automock]
pub trait TofuService: Send + Sync {
    async fn init(&self, params: InitParams) -> Result<(), ServiceInitError>;
    async fn list(&self) -> Result<Vec<Group>, ServiceListError>;
    async fn clean(&self) -> Result<(), ServiceCleanError>;
    async fn status(&self) -> Result<Vec<GroupStatus>, ServiceStatusError>;
}

pub struct Service<CLI, PROJECT, STATE, CONFIG>
where
    CLI: CLIPort + Send + Sync + 'static,
    PROJECT: ProjectConfigPort + Send + Sync + 'static,
    STATE: TFStatePort + Send + Sync + 'static,
    CONFIG: ConfigPort + Send + Sync + 'static,
{
    base_config: Arc<CONFIG>,
    project_config: Arc<PROJECT>,
    tofu_cli: Arc<CLI>,
    tf_state: Arc<STATE>,
}

impl<CLI, PROJECT, STATE, CONFIG> Service<CLI, PROJECT, STATE, CONFIG>
where
    CLI: CLIPort + Send + Sync + 'static,
    PROJECT: ProjectConfigPort + Send + Sync + 'static,
    STATE: TFStatePort + Send + Sync + 'static,
    CONFIG: ConfigPort + Send + Sync + 'static,
{
    pub fn new(
        base_config: CONFIG,
        project_config: PROJECT,
        tofu_cli: CLI,
        tf_state: STATE,
    ) -> Self {
        Self {
            base_config: Arc::new(base_config),
            project_config: Arc::new(project_config),
            tofu_cli: Arc::new(tofu_cli),
            tf_state: Arc::new(tf_state),
        }
    }
}

pub struct InitParams {
    pub group: String,
    pub state: String,
}

#[async_trait]
impl<CLI, PROJECT, STATE, CONFIG> TofuService for Service<CLI, PROJECT, STATE, CONFIG>
where
    CLI: CLIPort + Send + Sync + 'static,
    PROJECT: ProjectConfigPort + Send + Sync + 'static,
    STATE: TFStatePort + Send + Sync + 'static,
    CONFIG: ConfigPort + Send + Sync + 'static,
{
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
            ConfigStateHostType::Gitlab => {
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
