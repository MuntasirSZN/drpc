#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();
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
        match arrpc_bridge::Bridge::run(bus.clone(), None).await {
            Ok(b) => tracing::info!(port = b.port(), "started bridge server"),
            Err(e) => tracing::error!(error=?e, "failed to start bridge server"),
        }
    }
    #[cfg(feature = "process-scanning")]
    {
        let detectables = arrpc_core::load_detectables(false, 24);
        let backend = arrpc_process::LinuxBackend; // placeholder
        let scanner = arrpc_process::Scanner::new(backend, detectables, bus.clone());
        scanner.spawn();
        tracing::info!("process scanner started");
    }
    tokio::signal::ctrl_c().await.expect("install ctrl+c");
    Ok(())
}

fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = if std::env::var("ARRPC_DEBUG").ok().as_deref() == Some("1") {
        "debug"
    } else {
        "info"
    };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(filter));
    fmt().with_env_filter(filter).init();
}
