use serde::{Deserialize, Serialize};
use uuid::Uuid;

// IPC op codes (subset for stage 0-4)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[repr(i32)]
pub enum IpcOp {
    Handshake = 0,
    Frame = 1,
    Close = 2,
    Ping = 3,
    Pong = 4,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RpcCommand {
    Dispatch,
    SetActivity,
    InviteBrowser,
    GuildTemplateBrowser,
    DeepLink,
    ConnectionsCallback,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncomingFrame {
    pub cmd: RpcCommand,
    #[serde(default)]
    pub args: serde_json::Value,
    #[serde(default)]
    pub nonce: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutgoingFrame {
    pub cmd: RpcCommand,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evt: Option<String>,
    #[serde(default)]
    pub data: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nonce: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadyEvent {
    pub v: u8,
    pub config: ReadyConfig,
    pub user: MockUser,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadyConfig {
    pub cdn_host: String,
    pub api_endpoint: String,
    pub environment: String,
}

impl Default for ReadyConfig {
    fn default() -> Self {
        Self {
            cdn_host: "cdn.discordapp.com".into(),
            api_endpoint: "//discord.com/api".into(),
            environment: "production".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MockUser {
    pub id: String,
    pub username: String,
    pub discriminator: String,
    pub avatar: String,
    pub bot: bool,
}

impl Default for MockUser {
    fn default() -> Self {
        Self {
            id: "961950517370097704".into(),
            username: "drpc".into(),
            discriminator: "0000".into(),
            avatar: "a_39e73cb4db97d204c41e5328c85dc993".into(),
            bot: true,
        }
    }
}

// ---------------- Activity Models ----------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ActivityTimestamps {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ActivityAssets {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub large_image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub large_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub small_image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub small_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ActivityParty {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<(u32, u32)>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ActivitySecrets {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub join: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spectate: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#match: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityButton {
    pub label: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Activity {
    #[serde(default)]
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamps: Option<ActivityTimestamps>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assets: Option<ActivityAssets>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub party: Option<ActivityParty>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secrets: Option<ActivitySecrets>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buttons: Option<Vec<ActivityButton>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flags: Option<u32>,
    // Additional passthrough fields tolerated
    #[serde(flatten, default)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Activity {
    pub fn normalize(mut self) -> Self {
        // Convert timestamps heuristically to ms (if appears to be seconds)
        if let Some(ts) = self.timestamps.as_mut() {
            let now_ms = current_millis();
            if let Some(start) = ts.start.as_mut() {
                maybe_seconds_to_ms(start, now_ms);
            }
            if let Some(end) = ts.end.as_mut() {
                maybe_seconds_to_ms(end, now_ms);
            }
        }
        // instance flag -> flags bit 0
        if let Some(inst) = self.instance
            && inst
        {
            self.flags = Some(self.flags.unwrap_or(0) | 1);
        }
        if let Some(btns) = self.buttons.as_ref() {
            let labels: Vec<String> = btns.iter().map(|b| b.label.clone()).collect();
            let urls: Vec<String> = btns.iter().map(|b| b.url.clone()).collect();
            let meta = serde_json::json!({ "button_urls": urls });
            self.extra.insert("metadata".into(), meta);
            self.extra
                .insert("buttons".into(), serde_json::to_value(labels).unwrap());
            // Clear typed buttons so wire format uses label list
            self.buttons = None;
        }
        self
    }
}

fn maybe_seconds_to_ms(v: &mut u64, now_ms: u64) {
    // If value seems like seconds (significantly smaller magnitude) convert.
    if *v < now_ms / 100 {
        *v *= 1000;
    }
}

fn current_millis() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
