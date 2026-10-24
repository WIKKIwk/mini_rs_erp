//! macOS uses the explicitly built CoreWLAN helper, never the removed `airport`
//! utility. The helper fails closed when Location Services hides SSID/BSSID.

use std::{path::PathBuf, process::Stdio, sync::Mutex, time::Duration};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
    time::timeout,
};

use super::{OpenNetwork, WifiAdapter, WifiError};

const HELPER_TIMEOUT: Duration = Duration::from_secs(45);
const MAX_HELPER_OUTPUT: usize = 256 * 1024;

pub struct MacWifi {
    helper: PathBuf,
    /// Pin the selected Wi-Fi interface after the first successful read. Never
    /// fall back to another adapter when an explicitly selected one is absent.
    interface: Mutex<Option<String>>,
    attempted: Mutex<Option<(String, String)>>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ConnectionSnapshot {
    interface: String,
    ssid: String,
    bssid: String,
    security: u32,
}

#[derive(Deserialize)]
struct ScannedNetwork {
    ssid: String,
    bssid: String,
    signal: i32,
}

impl MacWifi {
    pub fn new(interface: Option<String>) -> Self {
        Self {
            helper: std::env::var_os("MINI_ERP_DOCTOR_WIFI_MAC_HELPER")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("tools/runtime/doctor_wifi")),
            interface: Mutex::new(interface),
            attempted: Mutex::new(None),
        }
    }

    async fn run(&self, operation: &str, args: &[&str]) -> Result<Value, WifiError> {
        let interface = self
            .interface
            .lock()
            .map_err(|_| WifiError::CommandFailed)?
            .clone();
        if interface
            .as_deref()
            .is_some_and(|name| !valid_interface(name))
        {
            return Err(WifiError::InvalidConfiguration);
        }
        let mut command = Command::new(&self.helper);
        command
            .arg(operation)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        if let Some(name) = &interface {
            command.args(["--interface", name]);
        }
        command.args(args);
        let mut child = command.spawn().map_err(io_error)?;
        let stdout = child.stdout.take().ok_or(WifiError::CommandFailed)?;
        let result = timeout(HELPER_TIMEOUT, async {
            tokio::try_join!(read_helper_output(stdout), async {
                child.wait().await.map_err(io_error)
            })
        })
        .await;
        let (bytes, status) = match result {
            Ok(Ok(output)) => output,
            result => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                return Err(match result {
                    Ok(Err(error)) => error,
                    Err(_) => WifiError::TimedOut,
                    Ok(Ok(_)) => unreachable!(),
                });
            }
        };
        let reply = parse_reply(&bytes, status.success())?;
        let selected = reply
            .get("interface")
            .and_then(Value::as_str)
            .filter(|name| valid_interface(name))
            .ok_or(WifiError::InvalidOutput)?;
        let mut pinned = self
            .interface
            .lock()
            .map_err(|_| WifiError::CommandFailed)?;
        if pinned.as_deref().is_some_and(|name| name != selected) {
            return Err(WifiError::InvalidOutput);
        }
        *pinned = Some(selected.to_owned());
        Ok(reply)
    }
}

fn io_error(error: std::io::Error) -> WifiError {
    match error.kind() {
        std::io::ErrorKind::NotFound => WifiError::Unavailable,
        std::io::ErrorKind::PermissionDenied => WifiError::PermissionDenied,
        _ => WifiError::CommandFailed,
    }
}

async fn read_helper_output(reader: impl AsyncRead + Unpin) -> Result<Vec<u8>, WifiError> {
    let mut bytes = Vec::new();
    reader
        .take((MAX_HELPER_OUTPUT + 1) as u64)
        .read_to_end(&mut bytes)
        .await
        .map_err(io_error)?;
    if bytes.len() > MAX_HELPER_OUTPUT {
        return Err(WifiError::InvalidOutput);
    }
    Ok(bytes)
}

fn valid_interface(name: &str) -> bool {
    !name.is_empty() && name.len() <= 32 && name.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

fn valid_ssid(ssid: &str) -> bool {
    !ssid.is_empty() && ssid.len() <= 32 && !ssid.contains('\0')
}

fn valid_bssid(bssid: &str) -> bool {
    bssid.len() == 17
        && bssid.split(':').count() == 6
        && bssid
            .split(':')
            .all(|part| part.len() == 2 && part.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn parse_reply(bytes: &[u8], process_succeeded: bool) -> Result<Value, WifiError> {
    if bytes.len() > MAX_HELPER_OUTPUT {
        return Err(WifiError::InvalidOutput);
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|_| WifiError::InvalidOutput)?;
    match value.get("ok").and_then(Value::as_bool) {
        Some(true) if process_succeeded => Ok(value),
        Some(true) => Err(WifiError::CommandFailed),
        Some(false) => Err(match value.get("error").and_then(Value::as_str) {
            Some("unsupported") => WifiError::Unsupported,
            Some("unavailable") => WifiError::Unavailable,
            Some("permission_denied") => WifiError::PermissionDenied,
            Some("timed_out") => WifiError::TimedOut,
            Some("invalid_configuration") => WifiError::InvalidConfiguration,
            Some("command_failed") => WifiError::CommandFailed,
            _ => WifiError::InvalidOutput,
        }),
        None => Err(WifiError::InvalidOutput),
    }
}

fn snapshot_from_json(value: &Value, selected: &str) -> Result<ConnectionSnapshot, WifiError> {
    let snapshot: ConnectionSnapshot =
        serde_json::from_value(value.clone()).map_err(|_| WifiError::InvalidOutput)?;
    if snapshot.interface != selected
        || !valid_interface(&snapshot.interface)
        || !valid_ssid(&snapshot.ssid)
        || !valid_bssid(&snapshot.bssid)
        || snapshot.security > 15
    {
        return Err(WifiError::InvalidOutput);
    }
    Ok(snapshot)
}

#[async_trait]
impl WifiAdapter for MacWifi {
    async fn current_connection(&self) -> Result<Option<String>, WifiError> {
        let reply = self.run("status", &[]).await?;
        let connection = reply.get("connection").ok_or(WifiError::InvalidOutput)?;
        if connection.is_null() {
            return Ok(None);
        }
        let selected = reply["interface"]
            .as_str()
            .ok_or(WifiError::InvalidOutput)?;
        let snapshot = snapshot_from_json(connection, selected)?;
        serde_json::to_string(&snapshot)
            .map(Some)
            .map_err(|_| WifiError::InvalidOutput)
    }

    async fn scan_open(&self) -> Result<Vec<OpenNetwork>, WifiError> {
        let reply = self.run("scan", &[]).await?;
        let networks: Vec<ScannedNetwork> = serde_json::from_value(
            reply
                .get("networks")
                .cloned()
                .ok_or(WifiError::InvalidOutput)?,
        )
        .map_err(|_| WifiError::InvalidOutput)?;
        if networks.len() > 512 {
            return Err(WifiError::InvalidOutput);
        }
        networks
            .into_iter()
            .map(|network| {
                if !valid_ssid(&network.ssid) || !valid_bssid(&network.bssid) {
                    return Err(WifiError::InvalidOutput);
                }
                Ok(OpenNetwork {
                    ssid: network.ssid,
                    bssid: network.bssid.to_ascii_lowercase(),
                    signal: network.signal,
                })
            })
            .collect()
    }

    async fn connect_open(&self, network: &OpenNetwork) -> Result<(), WifiError> {
        if !valid_ssid(&network.ssid) || !valid_bssid(&network.bssid) {
            return Err(WifiError::InvalidConfiguration);
        }
        // Record before execution: a timed-out helper may already have joined.
        *self
            .attempted
            .lock()
            .map_err(|_| WifiError::CommandFailed)? =
            Some((network.ssid.clone(), network.bssid.to_ascii_lowercase()));
        self.run(
            "connect",
            &["--ssid", &network.ssid, "--bssid", &network.bssid],
        )
        .await?;
        Ok(())
    }

    async fn disconnect_attempt(&self) -> Result<(), WifiError> {
        let attempted = self
            .attempted
            .lock()
            .map_err(|_| WifiError::CommandFailed)?
            .clone();
        let Some((ssid, bssid)) = attempted else {
            return Ok(());
        };
        // The helper only disconnects this exact open AP. An administrator's
        // intervening change to another connection is never disconnected.
        self.run("disconnect", &["--ssid", &ssid, "--bssid", &bssid])
            .await?;
        *self
            .attempted
            .lock()
            .map_err(|_| WifiError::CommandFailed)? = None;
        Ok(())
    }

    async fn restore_connection(&self, previous: &str) -> Result<(), WifiError> {
        let value: Value =
            serde_json::from_str(previous).map_err(|_| WifiError::InvalidConfiguration)?;
        let selected = self
            .interface
            .lock()
            .map_err(|_| WifiError::CommandFailed)?
            .clone()
            .ok_or(WifiError::InvalidConfiguration)?;
        let snapshot =
            snapshot_from_json(&value, &selected).map_err(|_| WifiError::InvalidConfiguration)?;
        self.run(
            "restore",
            &[
                "--ssid",
                &snapshot.ssid,
                "--bssid",
                &snapshot.bssid,
                "--security",
                &snapshot.security.to_string(),
            ],
        )
        .await?;
        *self
            .attempted
            .lock()
            .map_err(|_| WifiError::CommandFailed)? = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn helper_output_is_bounded_while_reading() {
        let valid = vec![b'x'; MAX_HELPER_OUTPUT];
        assert_eq!(
            read_helper_output(valid.as_slice()).await.unwrap().len(),
            MAX_HELPER_OUTPUT
        );
        let oversized = vec![b'x'; MAX_HELPER_OUTPUT + 128];
        assert!(matches!(
            read_helper_output(oversized.as_slice()).await,
            Err(WifiError::InvalidOutput)
        ));
    }

    #[test]
    fn helper_errors_keep_permission_and_unavailable_distinct() {
        assert!(matches!(
            parse_reply(br#"{"ok":false,"error":"permission_denied"}"#, false),
            Err(WifiError::PermissionDenied)
        ));
        assert!(matches!(
            parse_reply(br#"{"ok":false,"error":"unavailable"}"#, false),
            Err(WifiError::Unavailable)
        ));
        assert!(matches!(
            parse_reply(br#"{"ok":true,"interface":"en0"}"#, false),
            Err(WifiError::CommandFailed)
        ));
        assert!(matches!(
            parse_reply(br#"{"ok":false,"error":"unknown"}"#, false),
            Err(WifiError::InvalidOutput)
        ));
    }

    #[test]
    fn restore_snapshot_cannot_switch_interfaces_or_hide_network_identity() {
        let good = serde_json::json!({
            "interface": "en0", "ssid": "Original Wi-Fi", "bssid": "01:23:45:67:89:ab", "security": 4
        });
        assert!(snapshot_from_json(&good, "en0").is_ok());
        assert!(snapshot_from_json(&good, "en1").is_err());
        let mut bad = good;
        bad["bssid"] = Value::String("<redacted>".into());
        assert!(snapshot_from_json(&bad, "en0").is_err());
    }

    #[test]
    fn malformed_network_names_are_rejected_before_spawning() {
        assert!(valid_ssid("--ssid; $(ignored)"));
        assert!(!valid_ssid(""));
        assert!(!valid_ssid("bad\0ssid"));
        assert!(!valid_ssid(&"é".repeat(17)));
        assert!(valid_bssid("AA:bb:12:34:56:78"));
        assert!(!valid_bssid("AA:bb:12:34:56:78;"));
        assert!(!valid_interface("en0 --help"));
    }
}
