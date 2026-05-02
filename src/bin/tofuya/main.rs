use anyhow::anyhow;
use clap::{Parser, Subcommand};
use dirs::config_dir;
use std::process::exit;
use tofuya::domain::tofu::service::Service;
use tofuya::inbound::cli::CliHandler;
use tofuya::outbound::cli::CLI;
use tofuya::outbound::config::Config;
use tofuya::outbound::plugin::PluginAdapter;
use tofuya::outbound::project_config::ProjectConfig;
use tofuya::outbound::tfstate::TFStateAdapter;
use wasmtime::Engine;
use wasmtime::component::Linker;

const WASI_ADAPTER: &[u8] = include_bytes!("../../../wasi_snapshot_preview1.reactor.wasm");
const TOFUYA_INTERFACE: &[u8] = include_bytes!("../../../tofuya-plugin-interface.wasm");

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

    // wasm
    let mut config = wasmtime::Config::default();
    config.wasm_component_model(true);
    let engine = Engine::new(&config)?;
    let mut linker = Linker::new(&engine);
    wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
    wasmtime_wasi_http::p2::add_only_http_to_linker_async(&mut linker)?;
    let plugin = PluginAdapter::new(engine, linker);

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
                .map_err(|_| anyhow!("failed to list")),
            Commands::Clean => cli_handler
                .clean()
                .await
                .map_err(|_| anyhow!("failed to clean")),
            Commands::Status => cli_handler
                .status()
                .await
                .map_err(|_| anyhow!("failed to get status")),
            Commands::Embed => cli_handler
                .embed(TOFUYA_INTERFACE, WASI_ADAPTER)
                .await
                .map_err(|_| anyhow!("failed to embed")),
        },
    }
}
