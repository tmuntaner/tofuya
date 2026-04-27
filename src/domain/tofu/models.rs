use serde::Deserialize;
use std::path::PathBuf;
use url::Url;

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
