use crate::domain::tofu::service::{
    InitParams, ServiceCleanError, ServiceInitError, ServiceListError, ServiceStatusError,
    TofuService,
};
use comfy_table::presets::NOTHING;
use comfy_table::{Cell, Color, Table};
use std::sync::Arc;
use std::{fs, io};
use thiserror::Error;
use wit_component::{ComponentEncoder, StringEncoding};
use wit_parser::decoding::DecodedWasm;

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
    #[error(transparent)]
    AnyhowError(#[from] anyhow::Error),
    #[error(transparent)]
    IOError(#[from] io::Error),
    #[error("wit error")]
    WITError,
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

    pub async fn embed(&self, interface: &[u8], wasi_adapter: &[u8]) -> Result<(), CliError> {
        let mut core_wasm = fs::read("core.wasm")?;
        let (resolve, pkg_id) = match wit_component::decode(interface)? {
            DecodedWasm::WitPackage(res, id) => (res, id),
            _ => return Err(CliError::WITError),
        };

        let world_id = resolve.select_world(&[pkg_id], Some("tofuya-world"))?;
        wit_component::embed_component_metadata(
            &mut core_wasm,
            &resolve,
            world_id,
            StringEncoding::UTF8,
        )?;

        let component_bytes = ComponentEncoder::default()
            .module(&core_wasm)?
            .adapter("wasi_snapshot_preview1", wasi_adapter)?
            .encode()?;

        fs::write("component.wasm", component_bytes)?;

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
