use crate::domain::tofu::models::{ConfigStateHost, Group, StateTarget, StateType};
use async_trait::async_trait;
use mockall::automock;
use oci_client::ParseError;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use thiserror::Error;
use url::Url;

#[async_trait]
#[automock]
pub trait ConfigPort: Send + Sync {
    fn get_state_host(&self, req: Url) -> Option<ConfigStateHost>;
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// CLI Port
////////////////////////////////////////////////////////////////////////////////////////////////////

#[async_trait]
#[automock]
pub trait CLIPort: Send + Sync {
    fn clean(&self, params: CleanParams) -> Result<(), TofuCliError>;

    fn init_gitlab(&self, params: InitGitlabParams) -> Result<(), TofuCliError>;
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

////////////////////////////////////////////////////////////////////////////////////////////////////
// Project Config
////////////////////////////////////////////////////////////////////////////////////////////////////

#[async_trait]
#[automock]
pub trait ProjectConfigPort: Send + Sync {
    async fn list(&self) -> Result<Vec<Group>, ProjectListGroupsError>;
    async fn get_target(
        &self,
        target_group: String,
        target_state: String,
    ) -> Result<Option<StateTarget>, ProjectGetTargetError>;
    async fn state_from_address(
        &self,
        target_group: String,
        address: String,
    ) -> Result<Option<String>, ProjectGetStateFromAddressError>;
}

#[derive(Error, Debug)]
pub enum ProjectConfigError {
    #[error(transparent)]
    FSError(#[from] std::io::Error),
    #[error(transparent)]
    SerdeTomlError(#[from] toml::de::Error),
    #[error(transparent)]
    SerdeJsonError(#[from] serde_json::Error),
    #[error(transparent)]
    PluginError(#[from] PluginGetStatesError),
    #[error(transparent)]
    ParseError(#[from] url::ParseError),
    #[error("host not found")]
    HostNotFound,
}

#[derive(Error, Debug)]
pub enum ProjectListGroupsError {
    #[error(transparent)]
    ProjectErrorError(#[from] ProjectConfigError),
}

#[derive(Error, Debug)]
pub enum StateAddressError {
    #[error(transparent)]
    ParseError(#[from] url::ParseError),
}

#[derive(Error, Debug)]
pub enum ProjectGetTargetError {
    #[error("address not found")]
    AddressNotFound,
    #[error(transparent)]
    ParseError(#[from] StateAddressError),
    #[error(transparent)]
    ProjectErrorError(#[from] ProjectConfigError),
}

#[derive(Error, Debug)]
pub enum ProjectGetStateFromAddressError {
    #[error(transparent)]
    ProjectErrorError(#[from] ProjectConfigError),
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// TF State
////////////////////////////////////////////////////////////////////////////////////////////////////

#[async_trait]
#[automock]
pub trait TFStatePort: Send + Sync {
    fn parse(&self, tf_root: PathBuf) -> Result<Option<TFState>, TFStateParseError>;
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

////////////////////////////////////////////////////////////////////////////////////////////////////
// Plugin
////////////////////////////////////////////////////////////////////////////////////////////////////

#[async_trait]
#[automock]
pub trait PluginPort: Send + Sync {
    async fn get_states(
        &self,
        wasm_path: String,
        config: HashMap<String, String>,
    ) -> Result<Vec<String>, PluginGetStatesError>;
}

#[derive(Error, Debug)]
pub enum PluginGetStatesError {
    #[error(transparent)]
    WasmtimeError(#[from] wasmtime::Error),
    #[error(transparent)]
    FSError(#[from] std::io::Error),
    #[error("failed to call plugin: {0}")]
    PluginCallError(String),
    #[error("failed to start proxy")]
    PluginProxyError,
    #[error(transparent)]
    DownloadError(#[from] DownloaderPullError),
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Downloader
////////////////////////////////////////////////////////////////////////////////////////////////////

#[async_trait]
#[automock]
pub trait DownloaderPort: Send + Sync {
    async fn pull(&self, path: String) -> Result<PathBuf, DownloaderPullError>;
}

#[derive(Error, Debug)]
pub enum DownloaderPullError {
    #[error(transparent)]
    PathParseError(#[from] ParseError),
    #[error(transparent)]
    FSErrorError(#[from] std::io::Error),
    #[error(transparent)]
    DBError(#[from] DatabaseError),
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Database
////////////////////////////////////////////////////////////////////////////////////////////////////

#[async_trait]
#[automock]
pub trait DatabasePort: Send + Sync {
    async fn save(&self, reference: String, size: i64, hash: String) -> Result<(), DatabaseError>;

    async fn retrieve(&self, reference: String) -> Result<Option<String>, DatabaseError>;
}

#[derive(Error, Debug)]
pub enum DatabaseError {
    #[error(transparent)]
    SqliteError(#[from] rusqlite::Error),
    #[error("failed to get lock")]
    LockError,
    #[error("Background database task failed: {0}")]
    TaskError(#[from] tokio::task::JoinError),
    #[error(transparent)]
    MigrationError(#[from] rusqlite_migration::Error),
}
