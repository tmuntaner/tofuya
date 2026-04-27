use crate::domain::tofu::models::{Group, GroupState, StateTarget, StateType};
use crate::domain::tofu::ports::{
    ProjectConfigError, ProjectConfigPort, ProjectGetTargetError, StateAddressError,
};
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;
use url::Url;

#[derive(Deserialize, Clone, Default)]
pub struct ProjectConfig {
    state_groups: Vec<StateGroup>,
}

impl ProjectConfig {
    pub fn new(path: PathBuf) -> Result<Self, ProjectConfigError> {
        if !path.exists() {
            return Ok(Default::default());
        }

        let config_text = fs::read_to_string(path)?;
        let config: ProjectConfig = toml::from_str(config_text.as_str())?;

        Ok(config)
    }
}

impl ProjectConfigPort for ProjectConfig {
    fn list(&self) -> Vec<Group> {
        self.state_groups
            .iter()
            .map(|group| {
                let dir = group
                    .dir
                    .clone()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

                let states: Vec<GroupState> = group
                    .states
                    .iter()
                    .map(|state| GroupState {
                        name: state.clone(),
                    })
                    .collect();

                Group {
                    name: group.name.clone(),
                    states,
                    dir,
                }
            })
            .collect()
    }

    fn get_target(
        &self,
        target_group: String,
        target_state: String,
    ) -> Result<Option<StateTarget>, ProjectGetTargetError> {
        let state_group = self.state_groups.iter().find(|state_group| {
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

    fn state_from_address(&self, target_group: String, address: String) -> Option<String> {
        let state_group = self.state_groups.iter().find(|state_group| {
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
    // pub ext_state_command: Option<String>,
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

    #[test]
    fn test_config() {
        let config_dir = std::env::current_dir()
            .unwrap()
            .join("testdata")
            .join("project_configs")
            .join(".tofuya.toml");

        let config = ProjectConfig::new(config_dir).unwrap();
        assert_eq!(config.state_groups.len(), 1);
    }

    #[test]
    fn test_config_does_not_exist() {
        let config_dir = std::env::current_dir()
            .unwrap()
            .join("testdata")
            .join("project_configs")
            .join(".tofuya-missing.toml");

        let config = ProjectConfig::new(config_dir).unwrap();
        assert_eq!(config.state_groups.len(), 0);
    }

    #[test]
    fn test_config_get_target() {
        let config_dir = std::env::current_dir()
            .unwrap()
            .join("testdata")
            .join("project_configs")
            .join(".tofuya.toml");

        let config = ProjectConfig::new(config_dir).unwrap();
        let target = config
            .get_target(String::from("tofuya-main"), String::from("bar"))
            .unwrap()
            .unwrap();

        assert_eq!(
            target.url.as_str(),
            "https://gitlab.home.arpa/api/v4/projects/42/terraform/state/bar"
        );
        assert_eq!(StateType::OpenTofu, target.state_type);
    }

    #[test]
    fn test_config_get_target_malformed_state_name() {
        let config_dir = std::env::current_dir()
            .unwrap()
            .join("testdata")
            .join("project_configs")
            .join(".tofuya.toml");

        let config = ProjectConfig::new(config_dir).unwrap();
        let target = config.get_target(String::from("tofuya-main"), String::from("https://"));
        assert_eq!(true, target.is_err());
    }

    #[test]
    fn test_config_get_target_malformed_host() {
        let config = ProjectConfig {
            state_groups: vec![StateGroup {
                name: String::from("tofuya-main"),
                base_address: String::from("https://"),
                states: vec![String::from("foo")],
                ext_state_command: None,
                state_type: StateType::OpenTofu,
                dir: None,
            }],
        };
        let target = config.get_target(String::from("tofuya-main"), String::from("foo"));
        assert_eq!(true, target.is_err());
    }

    #[test]
    fn test_config_get_target_not_found() {
        let config_dir = std::env::current_dir()
            .unwrap()
            .join("testdata")
            .join("project_configs")
            .join(".tofuya-missing.toml");

        let config = ProjectConfig::new(config_dir).unwrap();
        assert_eq!(0, config.state_groups.len());
        let target = config
            .get_target(String::from("tofuya-main"), String::from("foo"))
            .unwrap();
        assert_eq!(true, target.is_none());
    }

    #[test]
    fn test_config_get_target_no_state() {
        let config_dir = std::env::current_dir()
            .unwrap()
            .join("testdata")
            .join("project_configs")
            .join(".tofuya.toml");

        let config = ProjectConfig::new(config_dir).unwrap();
        assert_eq!(1, config.state_groups.len());
        let target = config
            .get_target(String::from("tofuya-main"), String::from("foobar"))
            .unwrap();
        assert_eq!(true, target.is_none());
    }
}
