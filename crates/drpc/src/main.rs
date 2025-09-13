use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug, Clone)]
#[command(name = "drpc", version, about = "dRPC Rust server")]
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
    #[arg(long)]
    print_socket_paths: bool,
    #[arg(long, default_value_t = 0)]
    rest_port: u16,
}

#[derive(Debug, Default, serde::Deserialize)]
struct FileConfig {
    bridge_port: Option<u16>,
    detectables_ttl: Option<u64>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("crypto provider already set");
    let cli = Cli::parse();
    let file_cfg = load_config(cli.config.clone())?;
    init_tracing(&cli);
    let bus = drpc_core::EventBus::new();
    // Maintain an in-memory registry of active socket activities for graceful shutdown
    let registry = drpc_core::ActivityRegistry::new();
    {
        let bus_sub = bus.clone();
        let registry_clone = registry.clone();
        tokio::spawn(async move {
            let mut rx = bus_sub.subscribe();
            while let Some(evt) = rx.recv().await {
                match evt {
                    drpc_core::EventKind::ActivityUpdate { socket_id, payload } => {
                        registry_clone.set(socket_id, payload);
                    }
                    drpc_core::EventKind::Clear { socket_id } => {
                        registry_clone.clear(&socket_id);
                    }
                    drpc_core::EventKind::PrivacyRefresh => {
                        // no-op in registry
                    }
                }
            }
        });
    }
    #[cfg(feature = "ws")]
    {
        match drpc_ws::run_ws_server(bus.clone()).await {
            Ok(port) => tracing::info!(port, "started ws server"),
            Err(e) => tracing::error!(error=?e, "failed to start ws server"),
        }
    }
    #[cfg(feature = "ipc")]
    {
        match drpc_ipc::IpcServer::bind_with_bus(bus.clone()).await {
            Ok(server) => {
                tracing::info!(path=%server.path(), "started ipc server");
                if cli.print_socket_paths {
                    println!("{}", server.path());
                }
            }
            Err(e) => tracing::error!(error=?e, "failed to start ipc server"),
        }
    }
    #[cfg(feature = "bridge")]
    {
        let bridge_port = cli.bridge_port.or(file_cfg.bridge_port);
        match drpc_bridge::Bridge::run(bus.clone(), bridge_port).await {
            Ok(b) => tracing::info!(port = b.port(), "started bridge server"),
            Err(e) => tracing::error!(error=?e, "failed to start bridge server"),
        }
    }
    #[cfg(feature = "process-scanning")]
    {
        if !cli.no_process_scanning
            && std::env::var("DRPC_NO_PROCESS_SCANNING").ok().as_deref() != Some("1")
        {
            let ttl = cli
                .detectables_ttl
                .or(file_cfg.detectables_ttl)
                .unwrap_or(24);
            let detectables = match drpc_core::load_detectables_async(cli.refresh_detectables, ttl)
                .await
            {
                Ok(d) => d,
                Err(e) => {
                    tracing::error!(error=?e, "failed to load detectables; continuing with empty set");
                    drpc_core::Detectables::default()
                }
            };
            let backend = drpc_process::LinuxBackend; // placeholder
            let scanner = drpc_process::Scanner::new(backend, detectables, bus.clone());
            scanner.spawn();
            tracing::info!("process scanner started");
        } else {
            tracing::info!("process scanning disabled");
        }
    }
    #[cfg(feature = "rest")]
    {
        let rest_port = cli.rest_port;
        let reg_clone = registry.clone();
        let ttl = cli
            .detectables_ttl
            .or(file_cfg.detectables_ttl)
            .unwrap_or(24);
        // Load detectables (non-forced) for REST refresh endpoint even if scanning disabled
        let rest_detectables = match drpc_core::load_detectables_async(false, ttl).await {
            Ok(d) => Some(d),
            Err(e) => {
                tracing::warn!(error=?e, "rest detectables load failed; refresh endpoint limited");
                None
            }
        };
        match drpc_rest::run_rest(
            bus.clone(),
            reg_clone.into(),
            rest_detectables,
            ttl,
            rest_port,
        )
        .await
        {
            Ok(p) => tracing::info!(port = p, "started rest server"),
            Err(e) => tracing::error!(error=?e, "failed to start rest server"),
        }
    }
    tokio::signal::ctrl_c().await.expect("install ctrl+c");
    tracing::info!("shutdown signal received; broadcasting CLEAR to active sockets");
    for (socket_id, _activity) in registry.non_null() {
        bus.publish(drpc_core::EventKind::Clear { socket_id });
    }
    // Give subscribers a brief moment to flush messages
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    Ok(())
}

fn init_tracing(cli: &Cli) {
    use tracing_subscriber::{EnvFilter, fmt};
    let level = if std::env::var("DRPC_DEBUG").ok().as_deref() == Some("1") {
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
        let home = std::env::home_dir().unwrap_or_else(|| PathBuf::from("."));
        home.join(".drpc").join("config.toml")
    });
    if p.exists() {
        Ok(toml::from_str(&std::fs::read_to_string(p)?)?)
    } else {
        Ok(FileConfig::default())
    }
}
