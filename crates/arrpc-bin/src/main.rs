use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug, Clone)]
#[command(name = "arrpc", version, about = "arRPC Rust server")]
struct Cli {
    #[arg(long)]
    bridge_port: Option<u16>,
    #[arg(long)]
    refresh_detectables: bool,
    #[arg(long)]
    detectables_ttl: Option<u64>,
    #[arg(long)]
    no_process_scanning: bool,
    #[arg(long, value_parser=["pretty","json"], default_value="pretty")]
    log_format: String,
    #[arg(long)]
    config: Option<PathBuf>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct FileConfig {
    bridge_port: Option<u16>,
    detectables_ttl: Option<u64>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    arrpc_core::rustls::default_provider()
        .install_default()
        .expect("crypto provider already set");
    let cli = Cli::parse();
    let file_cfg = load_config(cli.config.clone())?;
    init_tracing(&cli);
    let bus = arrpc_core::EventBus::new();
    #[cfg(feature = "ws")]
    {
        match arrpc_ws::run_ws_server(bus.clone()).await {
            Ok(port) => tracing::info!(port, "started ws server"),
            Err(e) => tracing::error!(error=?e, "failed to start ws server"),
        }
    }
    #[cfg(feature = "ipc")]
    {
        match arrpc_ipc::IpcServer::bind_with_bus(bus.clone()).await {
            Ok(server) => tracing::info!(path=%server.path(), "started ipc server"),
            Err(e) => tracing::error!(error=?e, "failed to start ipc server"),
        }
    }
    #[cfg(feature = "bridge")]
    {
        let bridge_port = cli.bridge_port.or(file_cfg.bridge_port);
        match arrpc_bridge::Bridge::run(bus.clone(), bridge_port).await {
            Ok(b) => tracing::info!(port = b.port(), "started bridge server"),
            Err(e) => tracing::error!(error=?e, "failed to start bridge server"),
        }
    }
    #[cfg(feature = "process-scanning")]
    {
        if !cli.no_process_scanning
            && std::env::var("ARRPC_NO_PROCESS_SCANNING").ok().as_deref() != Some("1")
        {
            let ttl = cli
                .detectables_ttl
                .or(file_cfg.detectables_ttl)
                .unwrap_or(24);
            let detectables =
                arrpc_core::load_detectables_async(cli.refresh_detectables, ttl).await;
            let backend = arrpc_process::LinuxBackend; // placeholder
            let scanner = arrpc_process::Scanner::new(backend, detectables, bus.clone());
            scanner.spawn();
            tracing::info!("process scanner started");
        } else {
            tracing::info!("process scanning disabled");
        }
    }
    tokio::signal::ctrl_c().await.expect("install ctrl+c");
    Ok(())
}

fn init_tracing(cli: &Cli) {
    use tracing_subscriber::{fmt, EnvFilter};
    let level = if std::env::var("ARRPC_DEBUG").ok().as_deref() == Some("1") {
        "debug"
    } else {
        "info"
    };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));
    let builder = fmt().with_env_filter(filter);
    if cli.log_format == "json" {
        builder.json().init();
    } else {
        builder.init();
    }
}

fn load_config(path: Option<PathBuf>) -> anyhow::Result<FileConfig> {
    let p = path.unwrap_or_else(|| {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        let mut dir = PathBuf::from(home);
        dir.push(".arrpc");
        dir.push("config.toml");
        dir
    });
    if p.exists() {
        Ok(toml::from_str(&std::fs::read_to_string(p)?)?)
    } else {
        Ok(FileConfig::default())
    }
}
