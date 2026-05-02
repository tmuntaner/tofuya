use crate::domain::tofu::ports::{PluginGetStatesError, PluginPort};
use async_trait::async_trait;
use axum::Router;
use axum::body::Bytes;
use axum::http::{HeaderMap, Response, StatusCode};
use axum::response::IntoResponse;
use axum::routing::any;
use reqwest::Client;
use std::collections::HashMap;
use std::net::TcpListener;
use std::sync::Mutex;
use wasmtime::component::ResourceTable;
use wasmtime::{
    Engine, Store,
    component::{Component, Linker},
};
use wasmtime_wasi::{DirPerms, FilePerms, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};
use wasmtime_wasi_http::WasiHttpCtx;
use wasmtime_wasi_http::p2::{WasiHttpCtxView, WasiHttpView};

wasmtime::component::bindgen!({
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
        config: HashMap<String, String>,
    ) -> Result<Vec<String>, PluginGetStatesError> {
        let component = self.get_component(component_name)?;

        let wasi_http_ctx = WasiHttpCtx::new();
        let mut wasi_builder = WasiCtxBuilder::new();

        // file system access
        let host_dir = "./";
        std::fs::create_dir_all(host_dir)?;
        wasi_builder.preopened_dir(host_dir, "/", DirPerms::READ, FilePerms::READ)?;

        // network access
        let proxy_url = spawn_embedded_proxy()
            .await
            .map_err(|_| PluginGetStatesError::PluginProxyError)?;
        wasi_builder.env("TOFUYA_PROXY_URL", &proxy_url);

        // extra config
        for (key, value) in config {
            wasi_builder.env(key, value);
        }

        let state = PluginState {
            ctx: wasi_builder.build(),
            table: ResourceTable::new(),
            http: wasi_http_ctx,
        };

        let mut store = Store::new(&self.engine, state);
        let my_world = TofuyaWorld::instantiate_async(&mut store, &component, &self.linker).await?;
        let states = my_world
            .interface0
            .call_get_states(&mut store)
            .await?
            .map_err(|_| PluginGetStatesError::PluginCallError)?;

        Ok(states)
    }
}

pub struct PluginState {
    ctx: WasiCtx,
    table: ResourceTable,
    http: WasiHttpCtx,
}

impl WasiView for PluginState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.ctx,
            table: &mut self.table,
        }
    }
}

impl WasiHttpView for PluginState {
    fn http(&mut self) -> WasiHttpCtxView<'_> {
        WasiHttpCtxView {
            ctx: &mut self.http,
            table: &mut self.table,
            hooks: Default::default(),
        }
    }
}

async fn spawn_embedded_proxy() -> Result<String, Box<dyn std::error::Error>> {
    // by binding to 0, we get a random port for our proxy
    let std_listener = TcpListener::bind("127.0.0.1:0")?;
    std_listener.set_nonblocking(true)?;
    let port = std_listener.local_addr()?.port();
    let proxy_url = format!("http://127.0.0.1:{}", port);

    // convert to tokio tcp listener
    let std_listener = tokio::net::TcpListener::from_std(std_listener)?;
    let app = Router::new().fallback(any(proxy_handler));

    tokio::spawn(async move {
        if let Err(e) = axum::serve(std_listener, app).await {
            eprintln!("Embedded proxy crashed: {}", e);
        }
    });

    Ok(proxy_url)
}

async fn proxy_handler(
    headers: HeaderMap,
    method: axum::http::Method,
    body: Bytes,
) -> impl IntoResponse {
    // get the taget url from the proxy
    let target_url = match headers.get("X-Target-Url").and_then(|h| h.to_str().ok()) {
        Some(url) => url,
        None => return (StatusCode::BAD_REQUEST, "Missing X-Target-Url header").into_response(),
    };

    let client = Client::new();
    let mut req_builder = client.request(method, target_url).body(body);

    if let Some(auth) = headers.get("Authorization") {
        req_builder = req_builder.header("Authorization", auth);
    }

    if let Some(ct) = headers.get("Content-Type") {
        req_builder = req_builder.header("Content-Type", ct);
    }

    match req_builder.send().await {
        Ok(resp) => {
            let mut axum_resp = Response::builder().status(resp.status());

            // forward the headers
            for (k, v) in resp.headers() {
                axum_resp = axum_resp.header(k, v);
            }

            let resp_body = resp.bytes().await.unwrap_or_default();
            axum_resp.body(axum::body::Body::from(resp_body)).unwrap()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
