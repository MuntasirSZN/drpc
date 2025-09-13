use drpc_core::{
    Activity, EventBus, EventKind, IpcOp, MockUser, ReadyConfig, ReadyEvent, decode_frame,
    encode_frame,
};
use serde_json::json;
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{debug, info, info_span, warn};

#[derive(Debug, Error)]
pub enum IpcServerError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub struct IpcServer {
    path: String,
    #[allow(dead_code)] // for now
    bus: EventBus,
}

impl IpcServer {
    pub async fn bind_with_bus(bus: EventBus) -> Result<Self, IpcServerError> {
        let path: String;
        #[cfg(unix)]
        {
            let (listener, p) = scan_and_bind_ipc()?;
            path = p;
            info!(%path, "IPC listening");
            let bus_clone = bus.clone();
            tokio::spawn(async move {
                loop {
                    match listener.accept().await {
                        Ok((stream, addr)) => {
                            debug!(?addr, "ipc client connected");
                            let bus = bus_clone.clone();
                            tokio::spawn(handle_client(stream, bus));
                        }
                        Err(e) => {
                            warn!(error=?e, "accept failed");
                            break;
                        }
                    }
                }
            });
        }
        #[cfg(windows)]
        {
            use tokio::net::windows::named_pipe::ServerOptions;
            let (first_server, name) = scan_and_bind_ipc()?;
            path = name.clone();
            info!(path=%name, "IPC listening (Windows named pipe)");
            let bus_clone = bus.clone();
            tokio::spawn(async move {
                // Accept loop: connect initial instance, then create subsequent instances
                let mut next: Option<tokio::net::windows::named_pipe::NamedPipeServer> =
                    Some(first_server);
                loop {
                    let server = match next.take() {
                        Some(s) => s,
                        None => match ServerOptions::new()
                            .first_pipe_instance(false)
                            .create(&name)
                        {
                            Ok(s) => s,
                            Err(e) => {
                                warn!(error=?e, name, "failed to create pipe instance");
                                break;
                            }
                        },
                    };
                    match server.connect().await {
                        Ok(()) => {
                            debug!(path=%name, "ipc client connected (win)");
                            let bus = bus_clone.clone();
                            tokio::spawn(handle_client_windows(server, bus));
                            // continue loop with a fresh instance next iteration
                            next = None;
                        }
                        Err(e) => {
                            warn!(error=?e, "pipe connect failed");
                            // reuse by waiting for another connection
                            next = Some(server);
                        }
                    }
                }
            });
        }
        Ok(Self { path, bus })
    }

    pub fn path(&self) -> &str {
        &self.path
    }
}

async fn handle_client(mut stream: tokio::net::UnixStream, bus: EventBus) {
    let mut handshook = false;
    let socket_id = uuid::Uuid::new_v4().to_string();
    drpc_core::metrics::ACTIVE_CONNECTIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let span = info_span!("ipc_connection", %socket_id);
    let _enter = span.enter();
    loop {
        let mut header = [0u8; 8];
        if let Err(e) = stream.read_exact(&mut header).await {
            debug!(error=?e, "client closed");
            break;
        }
        let len = i32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
        let mut body = vec![0u8; len];
        if let Err(e) = stream.read_exact(&mut body).await {
            debug!(error=?e, "client closed mid-body");
            break;
        }
        if len > 64 * 1024 {
            debug!("payload too large; closing");
            break;
        }
        let mut full = Vec::from(header);
        full.extend_from_slice(&body);
        match decode_frame(&full) {
            Ok(frame) => {
                if !handshook {
                    if frame.op != IpcOp::Handshake {
                        debug!("expected handshake first");
                        break;
                    }
                    handshook = true;
                    debug!("handshake accepted");
                    // respond with READY DISPATCH full payload
                    let ready_data = ReadyEvent {
                        config: ReadyConfig::default(),
                        user: MockUser::default(),
                    };
                    let ready = json!({"cmd":"DISPATCH","evt":"READY","data": ready_data});
                    let buf = encode_frame(IpcOp::Frame, &ready);
                    let _ = stream.write_all(&buf).await;
                } else {
                    debug!(op=?frame.op, body=?frame.body, "frame");
                    match frame.op {
                        IpcOp::Ping => {
                            let buf = encode_frame(IpcOp::Pong, &json!({}));
                            let _ = stream.write_all(&buf).await;
                        }
                        IpcOp::Frame => {
                            if let Some(cmd) = frame.body.get("cmd").and_then(|c| c.as_str())
                                && cmd.eq_ignore_ascii_case("SET_ACTIVITY")
                                && let Some(args_root) = frame.body.get("args")
                                && let Some(act_json) = args_root.get("activity")
                                && let Ok(activity) =
                                    serde_json::from_value::<Activity>(act_json.clone())
                            {
                                let norm = activity.normalize();
                                let pid = args_root
                                    .get("pid")
                                    .and_then(|p| p.as_u64())
                                    .map(|p| p as u32);
                                let nonce = frame.body.get("nonce").cloned();
                                let out = json!({
                                    "cmd":"DISPATCH",
                                    "evt":"ACTIVITY_UPDATE",
                                    "data": {"activity": norm},
                                    "pid": pid,
                                    "nonce": nonce,
                                });
                                let buf = encode_frame(IpcOp::Frame, &out);
                                let _ = stream.write_all(&buf).await;
                                bus.publish(EventKind::ActivityUpdate {
                                    socket_id: socket_id.clone(),
                                    payload: serde_json::to_value(norm).unwrap_or(json!({})),
                                });
                            }
                        }
                        _ => {}
                    }
                }
            }
            Err(e) => {
                debug!(error=?e, "decode error");
                break;
            }
        }
    }
    bus.publish(EventKind::Clear { socket_id });
    drpc_core::metrics::ACTIVE_CONNECTIONS.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
}

#[cfg(windows)]
async fn handle_client_windows(
    mut stream: tokio::net::windows::named_pipe::NamedPipeServer,
    bus: EventBus,
) {
    let mut handshook = false;
    let socket_id = uuid::Uuid::new_v4().to_string();
    let span = info_span!("ipc_connection", %socket_id);
    let _enter = span.enter();
    loop {
        let mut header = [0u8; 8];
        if let Err(e) = stream.read_exact(&mut header).await {
            debug!(error=?e, "client closed");
            break;
        }
        let len = i32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
        let mut body = vec![0u8; len];
        if let Err(e) = stream.read_exact(&mut body).await {
            debug!(error=?e, "client closed mid-body");
            break;
        }
        if len > 64 * 1024 {
            debug!("payload too large; closing");
            break;
        }
        let mut full = Vec::from(header);
        full.extend_from_slice(&body);
        match decode_frame(&full) {
            Ok(frame) => {
                if !handshook {
                    if frame.op != IpcOp::Handshake {
                        debug!("expected handshake first");
                        break;
                    }
                    handshook = true;
                    debug!("handshake accepted");
                    // respond with READY DISPATCH full payload
                    let ready_data = ReadyEvent {
                        config: ReadyConfig::default(),
                        user: MockUser::default(),
                    };
                    let ready = json!({"cmd":"DISPATCH","evt":"READY","data": ready_data});
                    let buf = encode_frame(IpcOp::Frame, &ready);
                    let _ = stream.write_all(&buf).await;
                } else {
                    debug!(op=?frame.op, body=?frame.body, "frame");
                    match frame.op {
                        IpcOp::Ping => {
                            let buf = encode_frame(IpcOp::Pong, &json!({}));
                            let _ = stream.write_all(&buf).await;
                        }
                        IpcOp::Frame => {
                            if let Some(cmd) = frame.body.get("cmd").and_then(|c| c.as_str()) {
                                if cmd.eq_ignore_ascii_case("SET_ACTIVITY") {
                                    if let Some(args) =
                                        frame.body.get("args").and_then(|a| a.get("activity"))
                                    {
                                        if let Ok(activity) =
                                            serde_json::from_value::<Activity>(args.clone())
                                        {
                                            let norm = activity.normalize();
                                            let pid = frame
                                                .body
                                                .get("args")
                                                .and_then(|a| a.get("pid"))
                                                .and_then(|p| p.as_u64())
                                                .map(|p| p as u32);
                                            let nonce = frame.body.get("nonce").cloned();
                                            let out = json!({
                                                "cmd":"DISPATCH",
                                                "evt":"ACTIVITY_UPDATE",
                                                "data":{"activity":norm},
                                                "pid": pid,
                                                "nonce": nonce,
                                            });
                                            let buf = encode_frame(IpcOp::Frame, &out);
                                            let _ = stream.write_all(&buf).await;
                                            bus.publish(EventKind::ActivityUpdate {
                                                socket_id: socket_id.clone(),
                                                payload: serde_json::to_value(norm)
                                                    .unwrap_or(json!({})),
                                            });
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Err(e) => {
                debug!(error=?e, "decode error");
                break;
            }
        }
    }
    bus.publish(EventKind::Clear { socket_id });
}

#[cfg(unix)]
fn scan_and_bind_ipc() -> Result<(tokio::net::UnixListener, String), IpcServerError> {
    use std::path::PathBuf;
    let dirs = candidate_dirs();
    for dir in dirs {
        if std::fs::create_dir_all(&dir).is_err() {
            continue;
        }
        for i in 0..10 {
            // 0..9
            let mut p = PathBuf::from(&dir);
            p.push(format!("discord-ipc-{}", i));
            let path_str = p.to_string_lossy().to_string();
            if p.exists() {
                // test connect -> if works, occupied
                if std::os::unix::net::UnixStream::connect(&p).is_ok() {
                    continue;
                }
                // stale
                let _ = std::fs::remove_file(&p);
            }
            match tokio::net::UnixListener::bind(&p) {
                Ok(listener) => return Ok((listener, path_str)),
                Err(e) => {
                    warn!(path=%path_str, error=?e, "bind failed");
                    continue;
                }
            }
        }
    }
    Err(std::io::Error::new(std::io::ErrorKind::AddrNotAvailable, "no ipc path").into())
}

#[cfg(unix)]
fn candidate_dirs() -> Vec<String> {
    let mut v = Vec::new();
    for key in ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"] {
        if let Ok(val) = std::env::var(key)
            && !val.is_empty()
        {
            v.push(val);
        }
    }
    v.push("/tmp".into());
    v
}

// Windows named pipe scaffold (placeholder implementation)
#[cfg(windows)]
fn scan_and_bind_ipc()
-> Result<(tokio::net::windows::named_pipe::NamedPipeServer, String), IpcServerError> {
    use tokio::net::windows::named_pipe::ServerOptions;
    // Try discord-ipc-0..9 named pipes; pick first available
    for i in 0..10 {
        let name = format!("\\\\?\\pipe\\discord-ipc-{}", i);
        match ServerOptions::new().first_pipe_instance(true).create(&name) {
            Ok(server) => return Ok((server, name)),
            Err(_) => continue,
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AddrNotAvailable,
        "no named pipe available",
    )
    .into())
}

#[cfg(windows)]
fn candidate_dirs() -> Vec<String> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn handshake_and_ready() {
        let test_dir = format!(
            "/tmp/drpc-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        std::fs::create_dir_all(&test_dir).unwrap();
        unsafe {
            std::env::set_var("XDG_RUNTIME_DIR", &test_dir);
        }
        let server = IpcServer::bind_with_bus(drpc_core::EventBus::new())
            .await
            .expect("bind");
        let path = server.path();
        let mut client = tokio::net::UnixStream::connect(path)
            .await
            .expect("connect");
        let hs = json!({"v":1,"client_id":"123"});
        let buf = encode_frame(IpcOp::Handshake, &hs);
        client.write_all(&buf).await.unwrap();
        let mut header = [0u8; 8];
        client.read_exact(&mut header).await.unwrap();
        let len = i32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
        let mut body = vec![0u8; len];
        client.read_exact(&mut body).await.unwrap();
        let mut full = Vec::from(header);
        full.extend_from_slice(&body);
        let frame = decode_frame(&full).unwrap();
        assert_eq!(frame.op as i32, IpcOp::Frame as i32);
        assert!(frame.body.get("evt").and_then(|e| e.as_str()).unwrap_or("") == "READY");
    }

    #[tokio::test]
    async fn handshake_then_set_activity() {
        let test_dir = format!(
            "/tmp/drpc-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        std::fs::create_dir_all(&test_dir).unwrap();
        unsafe {
            std::env::set_var("XDG_RUNTIME_DIR", &test_dir);
        }
        let server = IpcServer::bind_with_bus(drpc_core::EventBus::new())
            .await
            .expect("bind");
        let path = server.path();
        let mut client = tokio::net::UnixStream::connect(path)
            .await
            .expect("connect");
        // Handshake
        let hs = serde_json::json!({"v":1,"client_id":"123"});
        let buf = encode_frame(IpcOp::Handshake, &hs);
        client.write_all(&buf).await.unwrap();
        // Read READY
        let mut header = [0u8; 8];
        client.read_exact(&mut header).await.unwrap();
        let len = i32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
        let mut body = vec![0u8; len];
        client.read_exact(&mut body).await.unwrap();
        // Send SET_ACTIVITY frame
        let set = serde_json::json!({
            "cmd":"SET_ACTIVITY",
            "args": {"activity": {"name": "TestGame"}}
        });
        let buf = encode_frame(IpcOp::Frame, &set);
        client.write_all(&buf).await.unwrap();
        // Read ACTIVITY_UPDATE
        let mut header2 = [0u8; 8];
        client.read_exact(&mut header2).await.unwrap();
        let len2 = i32::from_le_bytes(header2[4..8].try_into().unwrap()) as usize;
        let mut body2 = vec![0u8; len2];
        client.read_exact(&mut body2).await.unwrap();
        let mut full2 = Vec::from(header2);
        full2.extend_from_slice(&body2);
        let frame2 = decode_frame(&full2).unwrap();
        assert_eq!(frame2.op as i32, IpcOp::Frame as i32);
        let evt = frame2
            .body
            .get("evt")
            .and_then(|e| e.as_str())
            .unwrap_or("");
        assert_eq!(evt, "ACTIVITY_UPDATE");
        let name = frame2
            .body
            .get("data")
            .and_then(|d| d.get("activity"))
            .and_then(|a| a.get("name"))
            .and_then(|n| n.as_str())
            .unwrap_or("");
        assert_eq!(name, "TestGame");
    }
}
