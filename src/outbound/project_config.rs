use crate::domain::tofu::models::{Group, GroupState, StateTarget, StateType};
use crate::domain::tofu::ports::ProjectGetTargetError::InvalidDirectory;
use crate::domain::tofu::ports::{
    ConfigPort, PluginPort, ProjectConfigError, ProjectConfigPort, ProjectGetStateFromAddressError,
    ProjectGetTargetError, ProjectListGroupsError, StateAddressError,
};
use crate::outbound::config;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use url::Url;

#[derive(Clone, Default)]
pub struct ProjectConfig<PLUGIN>
where
    PLUGIN: PluginPort + Send + Sync + 'static,
{
    config: Config,
    base_config: config::Config,
    plugin: Arc<PLUGIN>,
}

impl<PLUGIN> ProjectConfig<PLUGIN>
where
    PLUGIN: PluginPort + Send + Sync + 'static,
{
    pub fn new(
        path: PathBuf,
        plugin: PLUGIN,
        base_config: config::Config,
    ) -> Result<Self, ProjectConfigError> {
        let plugin = Arc::new(plugin);
        let config = if !path.exists() {
            Self {
                config: Config::default(),
                base_config,
                plugin,
            }
        } else {
            let config_text = fs::read_to_string(path)?;
            let config = toml::from_str(config_text.as_str())?;

            Self {
                config,
                plugin,
                base_config,
            }
        };

        Ok(config)
    }

    async fn get_plugin_states(
        &self,
        group: &StateGroup,
        wasm_path: String,
    ) -> Result<Vec<String>, ProjectConfigError> {
        let mut url = Url::from_str(group.base_address.as_str())?;
        url.set_path("");
        let host = url.to_string();

        let state_host = self
            .base_config
            .get_state_host(url.clone())
            .ok_or(ProjectConfigError::HostNotFound)?;

        let mut plugin_config = HashMap::new();
        plugin_config.insert("STATE_HOST".to_string(), host);
        if let Some(auth_key) = state_host.gitlab_access_token {
            plugin_config.insert("GITLAB_ACCESS_TOKEN".to_string(), auth_key);
        }

        if let Some(wasm_config) = &group.wasm_config {
            let wasm_config = serde_json::to_string(wasm_config)?;
            plugin_config.insert("TOFUYA_PLUGIN_CONFIG".to_string(), wasm_config);
        }

        let states = self.plugin.get_states(wasm_path, plugin_config).await?;

        Ok(states)
    }

    async fn states(&self, group: &StateGroup) -> Result<Vec<String>, ProjectConfigError> {
        let states: Vec<String> = if let Some(wasm_file) = group.ext_wasm_file.clone() {
            self.get_plugin_states(group, wasm_file).await?
        } else {
            group.states.clone().unwrap_or_default()
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
            let state = self
                .states(state_group)
                .await?
                .into_iter()
                .find(|group_state| target_state.eq(group_state));

            // if we have a state, append it to the base URL
            if let Some(group_state) = state {
                let url = state_group
                    .state_address(group_state.clone())?
                    .ok_or(ProjectGetTargetError::AddressNotFound)?;

                let dir = state_group
                    .dir
                    .clone()
                    .map(PathBuf::from)
                    .ok_or_else(|| InvalidDirectory)?;
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

    async fn state_from_address(
        &self,
        target_group: String,
        address: String,
    ) -> Result<Option<String>, ProjectGetStateFromAddressError> {
        let state_group = self.config.state_groups.iter().find(|state_group| {
            let name = state_group.name.to_string();
            target_group.eq(&name)
        });

        if let Some(state_group) = state_group {
            let states = self.states(state_group).await?;

            for state in states.clone() {
                if let Ok(Some(state_address)) = state_group.state_address(state.clone())
                    && state_address.to_string().eq(&address)
                {
                    return Ok(Some(state));
                }
            }
        }

        Ok(None)
    }
}

#[derive(Deserialize, Clone)]
struct StateGroup {
    pub name: String,
    pub base_address: String,
    #[serde(default)]
    pub states: Option<Vec<String>>,
    pub ext_wasm_file: Option<String>,
    pub wasm_config: Option<HashMap<String, Value>>,
    pub state_type: StateType,
    pub dir: Option<String>,
}

impl StateGroup {
    fn state_address(&self, state: String) -> Result<Option<Url>, StateAddressError> {
        let base_address = Url::parse(self.base_address.as_str())?;
        let url = base_address.join(state.as_str())?;

        Ok(Some(url))
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::domain::tofu::ports::MockPluginPort;
    use mockall::predicate;
    use serde_json::json;

    #[test]
    fn test_config() {
        let plugin = MockPluginPort::new();
        let config_dir = std::env::current_dir()
            .unwrap()
            .join("testdata")
            .join("project_configs")
            .join(".tofuya.toml");

        let base_config = config::Config::default();
        let config = ProjectConfig::new(config_dir, plugin, base_config).unwrap();
        assert_eq!(config.config.state_groups.len(), 3);
    }

    #[test]
    fn test_config_does_not_exist() {
        let plugin = MockPluginPort::new();
        let config_dir = std::env::current_dir()
            .unwrap()
            .join("testdata")
            .join("project_configs")
            .join(".tofuya-missing.toml");

        let base_config = config::Config::default();
        let config = ProjectConfig::new(config_dir, plugin, base_config).unwrap();
        assert_eq!(config.config.state_groups.len(), 0);
    }

    #[tokio::test]
    async fn test_config_get_target_plugin() {
        let project_config = std::env::current_dir()
            .unwrap()
            .join("testdata")
            .join("project_configs")
            .join(".tofuya.toml");
        let config_dir = std::env::current_dir()
            .unwrap()
            .join("testdata")
            .join("config");

        let base_config = config::Config::new(Some(config_dir), None).unwrap();

        let mut plugin = MockPluginPort::new();
        plugin
            .expect_get_states()
            .with(
                predicate::eq(String::from(
                    "ghcr.io/tmuntaner/tofuya/plugin-gitlab-states:0.1.0",
                )),
                predicate::function(|actual_config: &HashMap<String, String>| {
                    if actual_config.get("STATE_HOST").map(|s| s.as_str())
                        != Some("https://gitlab-foo.home.arpa/")
                    {
                        return false;
                    }

                    if actual_config.get("GITLAB_ACCESS_TOKEN").map(|s| s.as_str()) != Some("bar") {
                        return false;
                    }

                    let json_matches = actual_config
                        .get("TOFUYA_PLUGIN_CONFIG")
                        .and_then(|s| serde_json::from_str::<Value>(s).ok())
                        .map(|actual_json| {
                            let expected_json = json!({
                                "gitlab_project": "foo/bar",
                                "regex_selector": "test.*-foo"
                            });
                            actual_json == expected_json
                        })
                        .unwrap_or(false);

                    if !json_matches {
                        return false;
                    }

                    true
                }),
            )
            .once()
            .once()
            .returning(|_, _| {
                let states = vec![String::from("bar")];
                Box::pin(async { Ok(states) })
            });

        let config = ProjectConfig::new(project_config, plugin, base_config).unwrap();
        let target = config
            .get_target(String::from("tofuya-foo"), String::from("bar"))
            .await
            .unwrap()
            .unwrap();

        assert_eq!(
            target.url.as_str(),
            "https://gitlab-foo.home.arpa/api/v4/projects/42/terraform/state/bar"
        );
        assert_eq!(StateType::OpenTofu, target.state_type);
    }

    #[tokio::test]
    async fn test_config_get_target_plugin_no_config() {
        let project_config = std::env::current_dir()
            .unwrap()
            .join("testdata")
            .join("project_configs")
            .join(".tofuya.toml");
        let config_dir = std::env::current_dir()
            .unwrap()
            .join("testdata")
            .join("config");

        let base_config = config::Config::new(Some(config_dir), None).unwrap();

        let mut plugin = MockPluginPort::new();
        plugin
            .expect_get_states()
            .with(
                predicate::eq(String::from(
                    "ghcr.io/tmuntaner/tofuya/plugin-gitlab-states:0.1.0",
                )),
                predicate::function(|actual_config: &HashMap<String, String>| {
                    if actual_config.get("STATE_HOST").map(|s| s.as_str())
                        != Some("https://gitlab-bar.home.arpa/")
                    {
                        return false;
                    }

                    if actual_config.get("GITLAB_ACCESS_TOKEN").map(|s| s.as_str())
                        != Some("foobar")
                    {
                        return false;
                    }

                    true
                }),
            )
            .once()
            .returning(|_, _| {
                let states = vec![String::from("foobar")];
                Box::pin(async { Ok(states) })
            });

        let config = ProjectConfig::new(project_config, plugin, base_config).unwrap();
        let target = config
            .get_target(String::from("tofuya-bar"), String::from("foobar"))
            .await
            .unwrap()
            .unwrap();

        assert_eq!(
            target.url.as_str(),
            "https://gitlab-bar.home.arpa/api/v4/projects/42/terraform/state/foobar"
        );
        assert_eq!(StateType::OpenTofu, target.state_type);
    }

    #[tokio::test]
    async fn test_config_get_target() {
        let plugin = MockPluginPort::new();
        let config_dir = std::env::current_dir()
            .unwrap()
            .join("testdata")
            .join("project_configs")
            .join(".tofuya.toml");

        let base_config = config::Config::default();
        let config = ProjectConfig::new(config_dir, plugin, base_config).unwrap();
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

        let base_config = config::Config::default();
        let config = ProjectConfig::new(config_dir, plugin, base_config).unwrap();
        let target = config
            .get_target(String::from("tofuya-main"), String::from("https://"))
            .await;
        assert_eq!(true, target.is_err());
    }

    #[tokio::test]
    async fn test_config_get_target_malformed_host() {
        let base_config = config::Config::default();
        let plugin = MockPluginPort::new();
        let config = ProjectConfig {
            config: Config {
                state_groups: vec![StateGroup {
                    name: String::from("tofuya-main"),
                    base_address: String::from("https://"),
                    states: Some(vec![String::from("foo")]),
                    ext_wasm_file: None,
                    wasm_config: None,
                    state_type: StateType::OpenTofu,
                    dir: None,
                }],
            },
            plugin: Arc::new(plugin),
            base_config,
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

        let base_config = config::Config::default();
        let config = ProjectConfig::new(config_dir, plugin, base_config).unwrap();
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

        let base_config = config::Config::default();
        let config = ProjectConfig::new(config_dir, plugin, base_config).unwrap();
        assert_eq!(3, config.config.state_groups.len());
        let target = config
            .get_target(String::from("tofuya-main"), String::from("foobar"))
            .await
            .unwrap();
        assert_eq!(true, target.is_none());
    }
}
