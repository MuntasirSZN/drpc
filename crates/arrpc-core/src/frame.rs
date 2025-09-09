use crate::protocol::IpcOp;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FrameError {
    #[error("buffer too small")]
    TooSmall,
    #[error("invalid op code {0}")]
    InvalidOp(i32),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawFrame<T = serde_json::Value> {
    pub op: IpcOp,
    pub body: T,
}

impl<T> RawFrame<T> {
    pub fn map<U, F: FnOnce(T) -> U>(self, f: F) -> RawFrame<U> {
        RawFrame {
            op: self.op,
            body: f(self.body),
        }
    }
}

pub fn encode_frame(op: IpcOp, body: &serde_json::Value) -> Vec<u8> {
    let json = serde_json::to_vec(body).expect("serialize frame body");
    let mut out = Vec::with_capacity(8 + json.len());
    out.extend_from_slice(&(op as i32).to_le_bytes());
    out.extend_from_slice(&(json.len() as i32).to_le_bytes());
    out.extend_from_slice(&json);
    out
}

pub fn decode_frame(buf: &[u8]) -> Result<RawFrame, FrameError> {
    if buf.len() < 8 {
        return Err(FrameError::TooSmall);
    }
    let op = i32::from_le_bytes(buf[0..4].try_into().unwrap());
    let len = i32::from_le_bytes(buf[4..8].try_into().unwrap());
    if !(0..=4).contains(&op) {
        return Err(FrameError::InvalidOp(op));
    }
    let op: IpcOp = unsafe { std::mem::transmute(op) }; // safe due to check
    let body_slice = &buf[8..];
    if body_slice.len() < len as usize {
        return Err(FrameError::TooSmall);
    }
    let val: serde_json::Value = serde_json::from_slice(&body_slice[..len as usize])?;
    Ok(RawFrame { op, body: val })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn round_trip() {
        let body = json!({"a":1});
        let buf = encode_frame(IpcOp::Ping, &body);
        let decoded = decode_frame(&buf).unwrap();
        assert_eq!(decoded.op as i32, IpcOp::Ping as i32);
        assert_eq!(decoded.body, body);
    }

    #[test]
    fn invalid_op() {
        let mut buf = encode_frame(IpcOp::Pong, &serde_json::json!({}));
        // Corrupt op to 99
        buf[0..4].copy_from_slice(&99i32.to_le_bytes());
        let err = decode_frame(&buf).unwrap_err();
        matches!(err, FrameError::InvalidOp(99));
    }

    #[test]
    fn truncated() {
        let body = json!({"k":true});
        let mut buf = encode_frame(IpcOp::Frame, &body);
        // remove last byte -> truncated body
        buf.pop();
        let err = decode_frame(&buf).unwrap_err();
        matches!(err, FrameError::TooSmall);
    }
}
