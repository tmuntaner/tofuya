use crate::domain::tofu::service::{
    InitParams, ServiceCleanError, ServiceInitError, ServiceListError, ServiceStatusError,
    TofuService,
};
use comfy_table::presets::NOTHING;
use comfy_table::{Cell, Color, Table};
use std::sync::Arc;
use thiserror::Error;

pub struct CliHandler<TOFU>
where
    TOFU: TofuService + Send + Sync + 'static,
{
    tofu_service: Arc<TOFU>,
}

#[derive(Debug, Error)]
pub enum CliError {
    #[error(transparent)]
    InitError(#[from] ServiceInitError),

    #[error(transparent)]
    CleanError(#[from] ServiceCleanError),

    #[error(transparent)]
    ListError(#[from] ServiceListError),

    #[error(transparent)]
    StatusError(#[from] ServiceStatusError),
}

impl<TOFU> CliHandler<TOFU>
where
    TOFU: TofuService + Send + Sync + 'static,
{
    pub fn new(tofu_service: TOFU) -> Self {
        Self {
            tofu_service: Arc::new(tofu_service),
        }
    }

    pub async fn init(&self, group: String, state: String) -> Result<(), CliError> {
        let params = InitParams { group, state };

        self.tofu_service.init(params).await?;
        Ok(())
    }

    pub async fn clean(&self) -> Result<(), CliError> {
        self.tofu_service.clean().await?;

        Ok(())
    }

    pub async fn list(&self) -> Result<(), CliError> {
        let groups = self.tofu_service.list().await?;

        let mut table = Table::new();
        table.load_preset(NOTHING);
        table.set_header(vec!["GROUP", "STATE"]);

        for group in groups {
            for state in group.states {
                table.add_row(vec![group.name.clone(), state.name.clone()]);
            }
        }

        println!("{table}");

        Ok(())
    }

    pub async fn status(&self) -> Result<(), CliError> {
        let statuses = self.tofu_service.status().await?;

        let mut table = Table::new();
        table.load_preset(NOTHING);
        table.set_header(vec!["GROUP", "STATE", "ADDRESS"]);

        for status in statuses {
            if status.address.is_none() {
                table.add_row(vec![
                    Cell::new(status.name),
                    Cell::new("Uninitialized").fg(Color::Yellow),
                    Cell::new("Uninitialized").fg(Color::Yellow),
                ]);
            } else if status.state.is_none() {
                table.add_row(vec![
                    Cell::new(status.name),
                    Cell::new("Unknown").fg(Color::Yellow),
                    Cell::new(status.address.unwrap_or_default()),
                ]);
            } else {
                table.add_row(vec![
                    status.name,
                    status.state.unwrap_or_default(),
                    status.address.unwrap_or_default(),
                ]);
            }
        }

        println!("{table}");

        Ok(())
    }
}
