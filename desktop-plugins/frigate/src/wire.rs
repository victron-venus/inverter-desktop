use crate::config::Configuration;
use serde::Deserialize;
use serde_json::Value;
use std::io::{BufRead, Write};
use tokio::sync::{mpsc, oneshot, watch};

pub const PLUGIN_ID: &str = "inverter-desktop.frigate";
pub const MAX_FRAME_BYTES: usize = 64 * 1024;

// Deliberately no Debug: Configuration contains plaintext credentials.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum HostFrame {
    Hello {
        protocol_version: u32,
        host_api_version: String,
        plugin_id: String,
    },
    Configuration {
        configuration: Configuration,
    },
    Shutdown {},
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    Running,
    Shutdown,
    Eof,
    Invalid,
}

struct Outbound {
    bytes: Vec<u8>,
    written: oneshot::Sender<Result<(), &'static str>>,
}

pub struct Output {
    sender: mpsc::Sender<Outbound>,
}

impl Output {
    pub async fn send(&self, value: Value) -> Result<(), &'static str> {
        let mut bytes = serde_json::to_vec(&value).map_err(|_| "cannot encode worker response")?;
        if bytes.len() >= MAX_FRAME_BYTES {
            return Err("worker response too large");
        }
        bytes.push(b'\n');
        let (written, result) = oneshot::channel();
        self.sender
            .send(Outbound { bytes, written })
            .await
            .map_err(|_| "host output closed")?;
        result.await.map_err(|_| "host output closed")?
    }
}

/// Own threads, rather than Tokio's blocking stdio pool, allow the process to
/// exit promptly when the host closes stdin while stdout is backpressured.
pub fn stdio() -> (
    mpsc::Receiver<HostFrame>,
    Output,
    watch::Receiver<StopReason>,
) {
    let (frames, incoming) = mpsc::channel(2);
    let (stop, stopped) = watch::channel(StopReason::Running);
    std::thread::spawn(move || {
        let mut input = std::io::BufReader::with_capacity(4096, std::io::stdin().lock());
        loop {
            match read_frame(&mut input) {
                Ok(Some(HostFrame::Shutdown {})) => {
                    let _ = stop.send(StopReason::Shutdown);
                    break;
                }
                Ok(Some(frame)) => {
                    if frames.try_send(frame).is_err() {
                        let _ = stop.send(StopReason::Invalid);
                        break;
                    }
                }
                Ok(None) => {
                    let _ = stop.send(StopReason::Eof);
                    break;
                }
                Err(_) => {
                    let _ = stop.send(StopReason::Invalid);
                    break;
                }
            }
        }
    });
    let (sender, mut outgoing) = mpsc::channel::<Outbound>(2);
    std::thread::spawn(move || {
        let mut output = std::io::stdout().lock();
        while let Some(frame) = outgoing.blocking_recv() {
            let result = output
                .write_all(&frame.bytes)
                .and_then(|()| output.flush())
                .map_err(|_| "host output closed");
            let failed = result.is_err();
            let _ = frame.written.send(result);
            if failed {
                break;
            }
        }
    });
    (incoming, Output { sender }, stopped)
}

pub fn read_frame<R: BufRead>(reader: &mut R) -> Result<Option<HostFrame>, &'static str> {
    let mut frame = Vec::new();
    loop {
        let chunk = reader.fill_buf().map_err(|_| "cannot read host frame")?;
        if chunk.is_empty() {
            return if frame.is_empty() {
                Ok(None)
            } else {
                Err("unterminated host frame")
            };
        }
        let newline = chunk.iter().position(|byte| *byte == b'\n');
        let length = newline.map_or(chunk.len(), |index| index + 1);
        if frame.len() + length > MAX_FRAME_BYTES {
            return Err("host frame too large");
        }
        frame.extend_from_slice(&chunk[..length]);
        reader.consume(length);
        if newline.is_some() {
            frame.pop();
            if frame.last() == Some(&b'\r') {
                frame.pop();
            }
            return serde_json::from_slice(&frame)
                .map(Some)
                .map_err(|_| "invalid host frame");
        }
    }
}

pub fn validate_hello(frame: HostFrame) -> Result<String, &'static str> {
    match frame {
        HostFrame::Hello {
            protocol_version: 1,
            host_api_version,
            plugin_id,
        } if plugin_id == PLUGIN_ID => {
            let version =
                semver::Version::parse(&host_api_version).map_err(|_| "unsupported host API")?;
            if !semver::VersionReq::parse("^1.2")
                .expect("fixed version requirement")
                .matches(&version)
            {
                return Err("unsupported host API");
            }
            Ok(host_api_version)
        }
        _ => Err("invalid worker handshake"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufReader, Cursor};

    #[test]
    fn framing_is_bounded_and_rejects_truncated_or_unknown_commands() {
        let mut reader = BufReader::with_capacity(3, Cursor::new(b"{\"type\":\"shutdown\"}\r\n"));
        assert!(matches!(
            read_frame(&mut reader).unwrap(),
            Some(HostFrame::Shutdown {})
        ));
        assert!(read_frame(&mut reader).unwrap().is_none());
        for bytes in [
            b"{\"type\":\"shutdown\"}".to_vec(),
            b"\n".to_vec(),
            b"{\"type\":\"shutdown\",\"extra\":1}\n".to_vec(),
            b"{\"type\":\"action\"}\n".to_vec(),
            vec![b' '; MAX_FRAME_BYTES + 1],
        ] {
            assert!(read_frame(&mut Cursor::new(bytes)).is_err());
        }
        let mut exact = vec![b' '; MAX_FRAME_BYTES - b"{\"type\":\"shutdown\"}\n".len()];
        exact.extend_from_slice(b"{\"type\":\"shutdown\"}\n");
        assert!(read_frame(&mut Cursor::new(exact)).is_ok());
    }

    #[test]
    fn accepts_only_compatible_host_api_and_own_identity() {
        for (api, accepted) in [
            ("1.2.0", true),
            ("1.3.1", true),
            ("1.1.0", false),
            ("2.0.0", false),
            ("1.2.0-beta.1", false),
            ("invalid", false),
        ] {
            assert_eq!(
                validate_hello(HostFrame::Hello {
                    protocol_version: 1,
                    host_api_version: api.into(),
                    plugin_id: PLUGIN_ID.into()
                })
                .is_ok(),
                accepted
            );
        }
        assert!(validate_hello(HostFrame::Hello {
            protocol_version: 1,
            host_api_version: "1.2.0".into(),
            plugin_id: "other.plugin".into()
        })
        .is_err());
        assert!(validate_hello(HostFrame::Hello {
            protocol_version: 2,
            host_api_version: "1.2.0".into(),
            plugin_id: PLUGIN_ID.into()
        })
        .is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn connection_waits_for_written_configuration_acknowledgement() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let (frames, mut incoming) = mpsc::channel(2);
        assert!(frames
            .send(HostFrame::Hello {
                protocol_version: 1,
                host_api_version: "1.2.0".into(),
                plugin_id: PLUGIN_ID.into(),
            })
            .await
            .is_ok());
        let configuration = serde_json::from_value(serde_json::json!({"revision":"revision-1","values":{"mqtt_host":"127.0.0.1","mqtt_port":listener.local_addr().unwrap().port()},"secrets":{}})).unwrap();
        assert!(frames
            .send(HostFrame::Configuration { configuration })
            .await
            .is_ok());
        let (sender, mut outgoing) = mpsc::channel::<Outbound>(2);
        let output = Output { sender };
        let session = tokio::spawn(async move { crate::session(&mut incoming, &output).await });
        let ready = outgoing.recv().await.unwrap();
        ready.written.send(Ok(())).unwrap();
        let acknowledgement = outgoing.recv().await.unwrap();
        let value: Value = serde_json::from_slice(&acknowledgement.bytes).unwrap();
        assert_eq!(
            value,
            serde_json::json!({"type":"configuration_ready","revision":"revision-1"})
        );
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), listener.accept())
                .await
                .is_err(),
            "no TCP connection before the acknowledgement is written"
        );
        acknowledgement.written.send(Ok(())).unwrap();
        let connecting = outgoing.recv().await.unwrap();
        connecting.written.send(Ok(())).unwrap();
        let _connection =
            tokio::time::timeout(std::time::Duration::from_secs(2), listener.accept())
                .await
                .unwrap()
                .unwrap();
        session.abort();
        let _ = session.await;
    }
}
