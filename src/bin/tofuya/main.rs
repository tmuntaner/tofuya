use anyhow::anyhow;
use clap::{Parser, Subcommand};
use dirs::config_dir;
use include_dir::{Dir, include_dir};
use rusqlite_migration::Migrations;
use std::process::exit;
use std::sync::LazyLock;
use tofuya::domain::tofu::service::Service;
use tofuya::inbound::cli::CliHandler;
use tofuya::outbound::cli::CLI;
use tofuya::outbound::config::Config;
use tofuya::outbound::db::DB;
use tofuya::outbound::downloader::Downloader;
use tofuya::outbound::plugin::PluginAdapter;
use tofuya::outbound::project_config::ProjectConfig;
use tofuya::outbound::tfstate::TFStateAdapter;
use wasmtime::Engine;
use wasmtime::component::Linker;

const WASI_ADAPTER: &[u8] = include_bytes!("../../../wasi_snapshot_preview1.reactor.wasm");
const TOFUYA_INTERFACE: &[u8] = include_bytes!("../../../tofuya-plugin-interface.wasm");

static MIGRATIONS_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/migrations");

// Define migrations. These are applied atomically.
static MIGRATIONS: LazyLock<Migrations<'static>> =
    LazyLock::new(|| Migrations::from_directory(&MIGRATIONS_DIR).unwrap());

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[arg(long)]
    config_path: Option<String>,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Init {
        #[clap(short, long)]
        group: String,

        #[clap(short, long)]
        state: String,
    },
    List,
    Clean,
    Status,
    Embed,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if let Err(e) = start(cli).await {
        println!("Error: {:#?}", e);
        exit(1);
    }
}

async fn start(cli: Cli) -> anyhow::Result<(), anyhow::Error> {
    // paths
    let config_dir = config_dir();
    let current_dir = std::env::current_dir().unwrap_or_default();
    let project_config_path = current_dir.join(".tofuya.toml");
    let data_dir = dirs::data_local_dir()
        .ok_or(anyhow!("failed to get data local dir"))?
        .join("tofuya");

    let plugin_path = data_dir.join("plugins");

    if !std::path::Path::new(&data_dir).exists() {
        std::fs::create_dir(&data_dir)?;
    }

    if !std::path::Path::new(&plugin_path).exists() {
        std::fs::create_dir(&plugin_path)?;
    }

    let db_path = data_dir.join("metadata.db");

    // database
    let db = DB::new(db_path, &MIGRATIONS)?;
    let downloader = Downloader::new(plugin_path, db);

    // wasm
    let mut config = wasmtime::Config::default();
    config.wasm_component_model(true);
    let engine = Engine::new(&config)?;
    let mut linker = Linker::new(&engine);
    wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
    wasmtime_wasi_http::p2::add_only_http_to_linker_async(&mut linker)?;
    let plugin = PluginAdapter::new(engine, linker, downloader);

    // configuration objects
    let base_config = Config::new(config_dir, cli.config_path)?;
    let project_config = ProjectConfig::new(project_config_path, plugin, base_config.clone())?;
    let tofu_cli = CLI::new();
    let tf_state = TFStateAdapter::new();

    // services
    let tofu_service = Service::new(base_config, project_config, tofu_cli, tf_state);

    // handlers
    let cli_handler = CliHandler::new(tofu_service);

    match cli.command {
        None => Ok(()),
        Some(subcommand) => match subcommand {
            Commands::Init { group, state } => cli_handler
                .init(group, state)
                .await
                .map_err(|err| anyhow!("failed to init: {}", err)),
            Commands::List => cli_handler
                .list()
                .await
                .map_err(|err| anyhow!("failed to list: {}", err)),
            Commands::Clean => cli_handler
                .clean()
                .await
                .map_err(|err| anyhow!("failed to clean: {}", err)),
            Commands::Status => cli_handler
                .status()
                .await
                .map_err(|err| anyhow!("failed to get status: {}", err)),
            Commands::Embed => cli_handler
                .embed(TOFUYA_INTERFACE, WASI_ADAPTER)
                .await
                .map_err(|err| anyhow!("failed to embed: {}", err)),
        },
    }
}
