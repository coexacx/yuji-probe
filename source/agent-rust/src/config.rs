use crate::Result;
use serde::{Deserialize, Serialize};
use std::{fs::File, io::Read, path::Path};
use tokio_tungstenite::tungstenite::http::HeaderValue;

#[derive(Clone, Deserialize, Serialize)]
pub struct Config {
    pub node_id: String,
    pub token: String,
    pub controller_url: String,
    pub ssh_port: u16,
}
impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let mut raw = Vec::new();
        File::open(path)
            .map_err(|_| "configuration unavailable")?
            .take(16385)
            .read_to_end(&mut raw)
            .map_err(|_| "configuration unavailable")?;
        if raw.len() > 16384 {
            return Err("configuration too large");
        }
        let c: Self = serde_json::from_slice(&raw).map_err(|_| "configuration invalid")?;
        c.validate()?;
        Ok(c)
    }
    pub fn validate(&self) -> Result<()> {
        let u = reqwest::Url::parse(&self.controller_url).map_err(|_| "controller URL invalid")?;
        if u.scheme() != "wss"
            || u.host_str().is_none()
            || !u.username().is_empty()
            || u.password().is_some()
            || u.query().is_some()
            || u.fragment().is_some()
            || u.path() != "/api/agent"
            || self.token.len() != 64
            || !(8..=64).contains(&self.node_id.len())
            || self.ssh_port == 0
            || HeaderValue::from_str(&self.token).is_err()
            || HeaderValue::from_str(&self.node_id).is_err()
        {
            return Err("configuration invalid");
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn good() -> Config {
        Config {
            node_id: "test-node-id".into(),
            token: "a".repeat(64),
            controller_url: "wss://example.com/api/agent".into(),
            ssh_port: 22,
        }
    }
    #[test]
    fn existing_config_accepted() {
        assert!(good().validate().is_ok());
    }
    #[test]
    fn unsafe_endpoint_rejected() {
        for url in [
            "ws://example.com/api/agent",
            "https://example.com/api/agent",
            "wss://user:pass@example.com/api/agent",
            "wss://example.com/api/agent?token=bad",
            "wss://example.com/api/agent#x",
            "wss://example.com/elsewhere",
        ] {
            let mut c = good();
            c.controller_url = url.into();
            assert!(c.validate().is_err(), "{url}");
        }
    }
    #[test]
    fn header_injection_and_bad_port_rejected() {
        let mut c = good();
        c.node_id = "bad-node\r\nHeader:x".into();
        assert!(c.validate().is_err());
        c = good();
        c.token = format!("{}\n", "a".repeat(63));
        assert!(c.validate().is_err());
        c = good();
        c.ssh_port = 0;
        assert!(c.validate().is_err());
    }
}
