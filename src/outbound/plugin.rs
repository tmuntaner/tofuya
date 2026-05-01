use crate::domain::tofu::ports::{PluginGetStatesError, PluginPort};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Mutex;
use wasmtime::component::ResourceTable;
use wasmtime::{
    Engine, Store,
    component::{Component, Linker},
};
use wasmtime_wasi::{DirPerms, FilePerms, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

wasmtime::component::bindgen!({
    world: "tofuya-world",
    imports: { default: async },
    exports: { default: async },
    require_store_data_send: true,
});

pub struct PluginAdapter {
    engine: Engine,
    linker: Linker<PluginState>,
    components: Mutex<HashMap<String, Component>>,
}

impl PluginAdapter {
    pub fn new(engine: Engine, linker: Linker<PluginState>) -> Self {
        let components = Mutex::new(HashMap::new());

        Self {
            engine,
            linker,
            components,
        }
    }

    fn get_component(&self, name: String) -> Result<Component, PluginGetStatesError> {
        let mut lock = self
            .components
            .lock()
            .map_err(|_| PluginGetStatesError::MutexLockError)?;

        let component = match lock.entry(name.clone()) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut().clone(),
            std::collections::hash_map::Entry::Vacant(entry) => {
                let component = Component::from_file(&self.engine, name)?;

                entry.insert(component).clone()
            }
        };

        Ok(component)
    }
}

#[async_trait]
impl PluginPort for PluginAdapter {
    async fn get_states(
        &self,
        component_name: String,
    ) -> Result<Vec<String>, PluginGetStatesError> {
        let component = self.get_component(component_name)?;

        let host_dir = "./cmd";
        std::fs::create_dir_all(host_dir)?;

        let mut wasi_builder = WasiCtxBuilder::new();
        wasi_builder.preopened_dir(host_dir, "/cmd", DirPerms::READ, FilePerms::READ)?;

        let state = PluginState {
            ctx: wasi_builder.build(),
            table: ResourceTable::new(),
        };

        let mut store = Store::new(&self.engine, state);
        let my_world = TofuyaWorld::instantiate_async(&mut store, &component, &self.linker).await?;
        let states: Vec<String> = my_world.call_get_states(&mut store).await?;

        Ok(states)
    }
}

pub struct PluginState {
    ctx: WasiCtx,
    table: ResourceTable,
}

impl WasiView for PluginState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.ctx,
            table: &mut self.table,
        }
    }
}
