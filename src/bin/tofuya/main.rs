use anyhow::anyhow;
use clap::{Parser, Subcommand};
use dirs::config_dir;
use std::process::exit;
use tofuya::core::config::Config;
use tofuya::domain::tofu::service::Service;
use tofuya::inbound::cli::CliHandler;
use tofuya::outbound::cli::CLI;
use tofuya::outbound::project_config::ProjectConfig;
use tofuya::outbound::tfstate::TFStateAdapter;
use tracing::error;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

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
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .init();

    if let Err(e) = start(cli).await {
        error!("Error: {:#?}", e);
        exit(1);
    }
}

async fn start(cli: Cli) -> anyhow::Result<(), anyhow::Error> {
    // paths
    let config_dir = config_dir();
    let current_dir = std::env::current_dir().unwrap_or_default();
    let project_config_path = current_dir.join(".tofuya.toml");

    // configuration objects
    let base_config = Config::new(config_dir, cli.config_path)?;
    let project_config = ProjectConfig::new(project_config_path)?;
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
        },
    }
}
