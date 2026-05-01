use crate::domain::tofu::models::{Group, GroupState, StateTarget, StateType};
use crate::domain::tofu::ports::{
    PluginPort, ProjectConfigError, ProjectConfigPort, ProjectGetTargetError,
    ProjectListGroupsError, StateAddressError,
};
use async_trait::async_trait;
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use url::Url;

#[derive(Clone, Default)]
pub struct ProjectConfig<PLUGIN>
where
    PLUGIN: PluginPort + Send + Sync + 'static,
{
    config: Config,
    plugin: Arc<PLUGIN>,
}

impl<PLUGIN> ProjectConfig<PLUGIN>
where
    PLUGIN: PluginPort + Send + Sync + 'static,
{
    pub fn new(path: PathBuf, plugin: PLUGIN) -> Result<Self, ProjectConfigError> {
        let plugin = Arc::new(plugin);
        let config = if !path.exists() {
            Self {
                config: Config::default(),
                plugin,
            }
        } else {
            let config_text = fs::read_to_string(path)?;
            let config = toml::from_str(config_text.as_str())?;

            Self { config, plugin }
        };

        Ok(config)
    }

    async fn get_plugin_states(
        &self,
        component_name: String,
    ) -> Result<Vec<String>, ProjectConfigError> {
        let states = self.plugin.get_states(component_name).await?;

        Ok(states)
    }

    async fn states(&self, group: &StateGroup) -> Result<Vec<String>, ProjectConfigError> {
        let states: Vec<String> = if let Some(wasm_file) = group.ext_wasm_file.clone() {
            self.get_plugin_states(wasm_file).await?
        } else {
            group.states.clone()
        };

        Ok(states)
    }
}

#[derive(Deserialize, Clone, Default)]
pub struct Config {
    state_groups: Vec<StateGroup>,
}

#[async_trait]
impl<PLUGIN> ProjectConfigPort for ProjectConfig<PLUGIN>
where
    PLUGIN: PluginPort + Send + Sync + 'static,
{
    async fn list(&self) -> Result<Vec<Group>, ProjectListGroupsError> {
        let mut groups: Vec<Group> = Vec::new();

        for group in &self.config.state_groups {
            let dir = group
                .dir
                .clone()
                .map(PathBuf::from)
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

            let states = self.states(group).await?;

            let states = states
                .iter()
                .map(|state| GroupState {
                    name: state.clone(),
                })
                .collect();

            groups.push(Group {
                name: group.name.clone(),
                states,
                dir,
            });
        }

        Ok(groups)
    }

    async fn get_target(
        &self,
        target_group: String,
        target_state: String,
    ) -> Result<Option<StateTarget>, ProjectGetTargetError> {
        let state_group = self.config.state_groups.iter().find(|state_group| {
            let name = state_group.name.to_string();
            target_group.eq(&name)
        });

        if let Some(state_group) = state_group {
            // find the state
            let state = state_group
                .states
                .iter()
                .find(|&group_state| target_state.eq(group_state));

            // if we have a state, append it to the base URL
            if let Some(group_state) = state {
                let url = state_group
                    .state_address(group_state.clone())?
                    .ok_or(ProjectGetTargetError::AddressNotFound)?;

                let dir = state_group
                    .dir
                    .clone()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
                let state_type = state_group.state_type.clone();

                return Ok(Some(StateTarget {
                    dir,
                    url,
                    state_type,
                }));
            }
        }

        Ok(None)
    }

    async fn state_from_address(&self, target_group: String, address: String) -> Option<String> {
        let state_group = self.config.state_groups.iter().find(|state_group| {
            let name = state_group.name.to_string();
            target_group.eq(&name)
        })?;

        for state in state_group.states.clone() {
            if let Ok(Some(state_address)) = state_group.state_address(state.clone())
                && state_address.to_string().eq(&address)
            {
                return Some(state);
            }
        }

        None
    }
}

#[derive(Deserialize, Clone, Default)]
struct StateGroup {
    pub name: String,
    pub base_address: String,
    pub states: Vec<String>,
    pub ext_wasm_file: Option<String>,
    pub state_type: StateType,
    pub dir: Option<String>,
}

impl StateGroup {
    fn state_address(&self, state: String) -> Result<Option<Url>, StateAddressError> {
        let base_address = Url::parse(self.base_address.as_str())?;
        let state = self.states.iter().find(|s| state.eq(s.to_owned()));
        match state {
            None => Ok(None),
            Some(state) => {
                let url = base_address.join(state)?;

                Ok(Some(url))
            }
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::domain::tofu::ports::MockPluginPort;

    #[test]
    fn test_config() {
        let plugin = MockPluginPort::new();
        let config_dir = std::env::current_dir()
            .unwrap()
            .join("testdata")
            .join("project_configs")
            .join(".tofuya.toml");

        let config = ProjectConfig::new(config_dir, plugin).unwrap();
        assert_eq!(config.config.state_groups.len(), 1);
    }

    #[test]
    fn test_config_does_not_exist() {
        let plugin = MockPluginPort::new();
        let config_dir = std::env::current_dir()
            .unwrap()
            .join("testdata")
            .join("project_configs")
            .join(".tofuya-missing.toml");

        let config = ProjectConfig::new(config_dir, plugin).unwrap();
        assert_eq!(config.config.state_groups.len(), 0);
    }

    #[tokio::test]
    async fn test_config_get_target() {
        let plugin = MockPluginPort::new();
        let config_dir = std::env::current_dir()
            .unwrap()
            .join("testdata")
            .join("project_configs")
            .join(".tofuya.toml");

        let config = ProjectConfig::new(config_dir, plugin).unwrap();
        let target = config
            .get_target(String::from("tofuya-main"), String::from("bar"))
            .await
            .unwrap()
            .unwrap();

        assert_eq!(
            target.url.as_str(),
            "https://gitlab.home.arpa/api/v4/projects/42/terraform/state/bar"
        );
        assert_eq!(StateType::OpenTofu, target.state_type);
    }

    #[tokio::test]
    async fn test_config_get_target_malformed_state_name() {
        let plugin = MockPluginPort::new();
        let config_dir = std::env::current_dir()
            .unwrap()
            .join("testdata")
            .join("project_configs")
            .join(".tofuya.toml");

        let config = ProjectConfig::new(config_dir, plugin).unwrap();
        let target = config
            .get_target(String::from("tofuya-main"), String::from("https://"))
            .await;
        assert_eq!(true, target.is_err());
    }

    #[tokio::test]
    async fn test_config_get_target_malformed_host() {
        let plugin = MockPluginPort::new();
        let config = ProjectConfig {
            config: Config {
                state_groups: vec![StateGroup {
                    name: String::from("tofuya-main"),
                    base_address: String::from("https://"),
                    states: vec![String::from("foo")],
                    ext_wasm_file: None,
                    state_type: StateType::OpenTofu,
                    dir: None,
                }],
            },
            plugin: Arc::new(plugin),
        };
        let target = config
            .get_target(String::from("tofuya-main"), String::from("foo"))
            .await;
        assert_eq!(true, target.is_err());
    }

    #[tokio::test]
    async fn test_config_get_target_not_found() {
        let plugin = MockPluginPort::new();
        let config_dir = std::env::current_dir()
            .unwrap()
            .join("testdata")
            .join("project_configs")
            .join(".tofuya-missing.toml");

        let config = ProjectConfig::new(config_dir, plugin).unwrap();
        assert_eq!(0, config.config.state_groups.len());
        let target = config
            .get_target(String::from("tofuya-main"), String::from("foo"))
            .await
            .unwrap();
        assert_eq!(true, target.is_none());
    }

    #[tokio::test]
    async fn test_config_get_target_no_state() {
        let plugin = MockPluginPort::new();
        let config_dir = std::env::current_dir()
            .unwrap()
            .join("testdata")
            .join("project_configs")
            .join(".tofuya.toml");

        let config = ProjectConfig::new(config_dir, plugin).unwrap();
        assert_eq!(1, config.config.state_groups.len());
        let target = config
            .get_target(String::from("tofuya-main"), String::from("foobar"))
            .await
            .unwrap();
        assert_eq!(true, target.is_none());
    }
}
