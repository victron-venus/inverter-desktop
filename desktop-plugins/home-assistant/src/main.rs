mod actions;
mod config;
mod network;
mod numeric;
mod state;

use config::Configuration;
use inverter_worker_protocol::{HostFrame, Output, StopReason};
use serde_json::json;
use std::{process::ExitCode, sync::Arc, time::Duration};
use tokio::sync::mpsc;

const PLUGIN_ID: &str = "inverter-desktop.home-assistant";
type Frame = HostFrame<Configuration>;

async fn next_frame(incoming: &mut mpsc::Receiver<Frame>) -> Result<Frame, &'static str> {
    tokio::time::timeout(Duration::from_secs(10), incoming.recv())
        .await
        .map_err(|_| "host handshake timed out")?
        .ok_or("host input closed")
}

fn validate_hello(frame: Frame) -> Result<String, &'static str> {
    match frame {
        HostFrame::Hello {
            protocol_version: 1,
            host_api_version,
            plugin_id,
        } if plugin_id == PLUGIN_ID => {
            let version =
                semver::Version::parse(&host_api_version).map_err(|_| "unsupported host API")?;
            if !semver::VersionReq::parse("^1.5")
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

async fn session(
    incoming: &mut mpsc::Receiver<Frame>,
    output: &Output,
) -> Result<(), &'static str> {
    let api = validate_hello(next_frame(incoming).await?)?;
    output.send(json!({"type":"ready","protocol_version":1,"host_api_version":api,"plugin_id":PLUGIN_ID})).await?;
    let HostFrame::Configuration { configuration } = next_frame(incoming).await? else {
        return Err("configuration required");
    };
    let revision = configuration.revision.clone();
    let configuration = Arc::new(configuration.validate()?);
    output
        .send(json!({"type":"configuration_ready","revision":revision}))
        .await?;
    // Network work starts only after the configuration acknowledgement flushes.
    let book = state::Book::new(
        &configuration.entities,
        &configuration.actions(),
        &configuration.inputs(),
    );
    tokio::select! {
        result=actions::run(incoming,output,configuration.clone(),book.clone())=>result,
        result=state::publish(book.clone(),output.clone())=>result,
        result=network::run(configuration,book)=>result,
    }
}

async fn run() -> Result<(), &'static str> {
    let (mut incoming, output, mut stopped) = inverter_worker_protocol::stdio::<Configuration>();
    let result = tokio::select! {
        biased;
        reason=stopped.wait_for(|reason| *reason!=StopReason::Running)=>match reason {
            Ok(reason) if *reason==StopReason::Shutdown || *reason==StopReason::Eof => Ok(()),
            _=>Err("invalid host input"),
        },
        result=session(&mut incoming,&output)=>result,
    };
    if matches!(*stopped.borrow(), StopReason::Shutdown | StopReason::Eof) {
        Ok(())
    } else {
        result
    }
}

fn main() -> ExitCode {
    std::panic::set_hook(Box::new(|_| eprintln!("Home Assistant worker failed")));
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            eprintln!("Home Assistant worker could not start");
            return ExitCode::FAILURE;
        }
    };
    let result = runtime.block_on(run());
    runtime.shutdown_timeout(Duration::from_millis(100));
    if result.is_err() {
        eprintln!("Home Assistant worker session failed");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_own_identity_and_stable_compatible_api_are_accepted() {
        for (api, accepted) in [
            ("1.5.0", true),
            ("1.4.0", false),
            ("1.3.0", false),
            ("1.9.0", true),
            ("1.2.0", false),
            ("2.0.0", false),
            ("1.5.0-beta.1", false),
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
            host_api_version: "1.5.0".into(),
            plugin_id: "other.plugin".into()
        })
        .is_err());
        assert!(validate_hello(HostFrame::Hello {
            protocol_version: 2,
            host_api_version: "1.5.0".into(),
            plugin_id: PLUGIN_ID.into()
        })
        .is_err());
    }
}

#[cfg(test)]
mod configuration_flush_tests {
    use super::*;
    use serde_json::Value;
    use std::io::{self, Write};
    use tokio::sync::oneshot;

    struct PendingFlush {
        bytes: Vec<u8>,
        release: std::sync::mpsc::SyncSender<()>,
    }

    struct ControlledWriter {
        bytes: Vec<u8>,
        flushed: mpsc::Sender<PendingFlush>,
        finished: Option<oneshot::Sender<()>>,
    }

    impl Write for ControlledWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.bytes.len() + bytes.len() > inverter_worker_protocol::MAX_FRAME_BYTES {
                return Err(io::Error::from(io::ErrorKind::InvalidData));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            let (release, released) = std::sync::mpsc::sync_channel(1);
            self.flushed
                .blocking_send(PendingFlush {
                    bytes: std::mem::take(&mut self.bytes),
                    release,
                })
                .map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))?;
            released
                .recv()
                .map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))
        }
    }

    impl Drop for ControlledWriter {
        fn drop(&mut self) {
            if let Some(finished) = self.finished.take() {
                let _ = finished.send(());
            }
        }
    }

    async fn pending_flush(outgoing: &mut mpsc::Receiver<PendingFlush>) -> PendingFlush {
        tokio::time::timeout(Duration::from_secs(2), outgoing.recv())
            .await
            .unwrap()
            .expect("worker must reach the controlled writer")
    }

    #[tokio::test(flavor = "current_thread")]
    async fn ha_network_waits_for_actual_configuration_flush() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let (frames, mut incoming) = mpsc::channel(2);
        assert!(frames
            .send(HostFrame::Hello {
                protocol_version: 1,
                host_api_version: "1.5.0".into(),
                plugin_id: PLUGIN_ID.into(),
            })
            .await
            .is_ok());
        let configuration = serde_json::from_value(json!({
            "revision":"flush-revision",
            "values":{"ha_base_url":format!("http://{}/ha/",listener.local_addr().unwrap())},
            "secrets":{"ha_token":"disposable-flush-fixture-token"}
        }))
        .unwrap();
        assert!(frames
            .send(HostFrame::Configuration { configuration })
            .await
            .is_ok());
        let (flushed, mut outgoing) = mpsc::channel(2);
        let (finished, writer_finished) = oneshot::channel();
        let output = Output::with_writer(ControlledWriter {
            bytes: Vec::new(),
            flushed,
            finished: Some(finished),
        });
        let running = tokio::spawn(async move { session(&mut incoming, &output).await });
        let ready = pending_flush(&mut outgoing).await;
        let ready_value: Value = serde_json::from_slice(&ready.bytes).unwrap();
        assert_eq!(ready_value["type"], "ready");
        assert_eq!(ready_value["plugin_id"], PLUGIN_ID);
        assert!(
            tokio::time::timeout(Duration::from_millis(100), listener.accept())
                .await
                .is_err()
        );
        ready.release.send(()).unwrap();

        let acknowledgement = pending_flush(&mut outgoing).await;
        let value: Value = serde_json::from_slice(&acknowledgement.bytes).unwrap();
        assert_eq!(
            value,
            json!({"type":"configuration_ready","revision":"flush-revision"})
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(100), listener.accept())
                .await
                .is_err(),
            "HA must not connect while the configuration acknowledgement is unflushed"
        );
        acknowledgement.release.send(()).unwrap();
        let _connection = tokio::time::timeout(Duration::from_secs(2), listener.accept())
            .await
            .unwrap()
            .unwrap();
        running.abort();
        assert!(running.await.unwrap_err().is_cancelled());
        // Closing the bounded observer releases any optional contribution flush
        // and proves the supplied writer thread has dropped its fixture owner.
        drop(outgoing);
        tokio::time::timeout(Duration::from_secs(2), writer_finished)
            .await
            .unwrap()
            .unwrap();
    }
}
