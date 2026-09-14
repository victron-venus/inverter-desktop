use super::*;
use serde_json::json;
use std::io::{self, BufReader, Cursor};
use std::time::Duration;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Configuration {
    token: String,
}

#[test]
fn framing_is_bounded_and_rejects_truncated_or_unknown_commands() {
    let mut reader = BufReader::with_capacity(3, Cursor::new(b"{\"type\":\"shutdown\"}\r\n"));
    assert!(matches!(
        read_frame::<_, Configuration>(&mut reader).unwrap(),
        Some(HostFrame::Shutdown {})
    ));
    assert!(read_frame::<_, Configuration>(&mut reader)
        .unwrap()
        .is_none());
    for bytes in [
        b"{\"type\":\"shutdown\"}".to_vec(),
        b"\n".to_vec(),
        b"{\"type\":\"shutdown\",\"extra\":1}\n".to_vec(),
        b"{\"type\":\"action\"}\n".to_vec(),
        vec![b' '; MAX_FRAME_BYTES + 1],
    ] {
        assert!(read_frame::<_, Configuration>(&mut Cursor::new(bytes)).is_err());
    }
    let mut exact = vec![b' '; MAX_FRAME_BYTES - b"{\"type\":\"shutdown\"}\n".len()];
    exact.extend_from_slice(b"{\"type\":\"shutdown\"}\n");
    assert!(read_frame::<_, Configuration>(&mut Cursor::new(exact)).is_ok());
}

#[test]
fn configuration_deserialization_remains_owned_by_each_worker() {
    let valid = b"{\"type\":\"configuration\",\"configuration\":{\"token\":\"fixture-secret\"}}\n";
    let frame = read_frame::<_, Configuration>(&mut Cursor::new(valid)).unwrap();
    match frame {
        Some(HostFrame::Configuration { configuration }) => {
            assert_eq!(configuration.token, "fixture-secret")
        }
        _ => panic!("expected typed configuration"),
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct OtherConfiguration {
        enabled: bool,
    }
    assert!(matches!(
        read_frame::<_, OtherConfiguration>(&mut Cursor::new(valid)),
        Err("invalid host frame")
    ));
    let other = b"{\"type\":\"configuration\",\"configuration\":{\"enabled\":true}}\n";
    match read_frame::<_, OtherConfiguration>(&mut Cursor::new(other)).unwrap() {
        Some(HostFrame::Configuration { configuration }) => assert!(configuration.enabled),
        _ => panic!("expected worker-specific configuration"),
    }
    let unknown = b"{\"type\":\"configuration\",\"configuration\":{\"token\":\"fixture-secret\",\"extra\":true}}\n";
    assert!(matches!(
        read_frame::<_, Configuration>(&mut Cursor::new(unknown)),
        Err("invalid host frame")
    ));
}

#[test]
fn reader_shutdown_bypasses_a_full_bounded_command_queue() {
    let hello = b"{\"type\":\"hello\",\"protocol_version\":1,\"host_api_version\":\"1.3.0\",\"plugin_id\":\"any.worker\"}\n";
    let mut input = hello.repeat(QUEUE_CAPACITY);
    input.extend_from_slice(b"{\"type\":\"shutdown\"}\n");
    let (sender, incoming) = mpsc::channel::<HostFrame<Configuration>>(QUEUE_CAPACITY);
    let (stop, stopped) = watch::channel(StopReason::Running);
    read_frames(Cursor::new(input), sender, stop);
    assert!(matches!(*stopped.borrow(), StopReason::Shutdown));
    assert_eq!(incoming.len(), QUEUE_CAPACITY);

    let (sender, incoming) = mpsc::channel::<HostFrame<Configuration>>(QUEUE_CAPACITY);
    let (stop, stopped) = watch::channel(StopReason::Running);
    read_frames(Cursor::new(hello.repeat(QUEUE_CAPACITY + 1)), sender, stop);
    assert!(matches!(*stopped.borrow(), StopReason::Invalid));
    assert_eq!(incoming.len(), QUEUE_CAPACITY);
}

#[test]
fn reader_distinguishes_eof_and_invalid_input_without_secret_diagnostics() {
    for (input, expected) in [
        (b"".as_slice(), StopReason::Eof),
        (b"{\"type\":\"shutdown\"}".as_slice(), StopReason::Invalid),
        (
            b"{\"type\":\"shutdown\",\"token\":\"fixture-secret\"}\n".as_slice(),
            StopReason::Invalid,
        ),
    ] {
        let (sender, mut incoming) = mpsc::channel::<HostFrame<Configuration>>(QUEUE_CAPACITY);
        let (stop, stopped) = watch::channel(StopReason::Running);
        read_frames(Cursor::new(input), sender, stop);
        assert!(*stopped.borrow() == expected);
        assert!(matches!(
            incoming.try_recv(),
            Err(mpsc::error::TryRecvError::Disconnected)
        ));
    }
}

#[test]
fn cloned_outputs_share_the_same_bounded_queue_and_encoding_limit() {
    let (sender, mut outgoing) = mpsc::channel(QUEUE_CAPACITY);
    let output = Output { sender };
    let clone = output.clone();
    let frame = json!({"type":"contributions","contributions":[]});
    assert!(output.try_send(frame.clone()).unwrap());
    assert!(clone.try_send(frame.clone()).unwrap());
    assert!(!output.try_send(frame.clone()).unwrap());
    let queued = outgoing.try_recv().unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&queued.bytes).unwrap(),
        frame
    );
    assert!(clone.try_send(frame.clone()).unwrap());
    let exact = encode_frame(Value::String("x".repeat(MAX_FRAME_BYTES - 3))).unwrap();
    assert_eq!(exact.len(), MAX_FRAME_BYTES);
    assert_eq!(exact.last(), Some(&b'\n'));
    assert!(encode_frame(Value::String("x".repeat(MAX_FRAME_BYTES - 2))).is_err());
    drop(outgoing);
    assert!(clone.try_send(frame).is_err());
}

struct PendingFlush {
    bytes: Vec<u8>,
    release: std::sync::mpsc::SyncSender<io::Result<()>>,
}

struct ControlledWriter {
    bytes: Vec<u8>,
    flushed: mpsc::Sender<PendingFlush>,
}

impl Write for ControlledWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
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
            .unwrap_or_else(|_| Err(io::ErrorKind::BrokenPipe.into()))
    }
}

#[tokio::test(flavor = "current_thread")]
async fn send_acknowledges_only_a_successful_completed_flush() {
    let (flushed, mut pending) = mpsc::channel(2);
    let output = Output::with_writer(ControlledWriter {
        bytes: Vec::new(),
        flushed,
    });
    let sender = output.clone();
    let send = tokio::spawn(async move {
        sender
            .send(json!({"type":"configuration_ready","revision":"fixture"}))
            .await
    });
    let flushing = tokio::time::timeout(Duration::from_secs(2), pending.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&flushing.bytes).unwrap(),
        json!({"type":"configuration_ready","revision":"fixture"})
    );
    assert!(
        !send.is_finished(),
        "accepted bytes do not imply flushed acknowledgement"
    );
    flushing.release.send(Ok(())).unwrap();
    assert!(tokio::time::timeout(Duration::from_secs(2), send)
        .await
        .unwrap()
        .unwrap()
        .is_ok());

    let sender = output.clone();
    let send = tokio::spawn(async move { sender.send(json!({"type":"ready"})).await });
    let flushing = tokio::time::timeout(Duration::from_secs(2), pending.recv())
        .await
        .unwrap()
        .unwrap();
    flushing
        .release
        .send(Err(io::Error::other("fixture-secret")))
        .unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), send)
            .await
            .unwrap()
            .unwrap(),
        Err("host output closed")
    );
    assert_eq!(
        output.send(json!({"type":"ready"})).await,
        Err("host output closed")
    );
}

#[test]
fn write_failure_is_reported_without_the_underlying_error_contents() {
    struct FailingWriter;
    impl Write for FailingWriter {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("fixture-secret"))
        }
        fn flush(&mut self) -> io::Result<()> {
            panic!("flush must not run after a failed write")
        }
    }
    let (sender, outgoing) = mpsc::channel(QUEUE_CAPACITY);
    let (written, result) = oneshot::channel();
    sender
        .try_send(Outbound {
            bytes: b"{}\n".to_vec(),
            written,
        })
        .ok()
        .unwrap();
    write_frames(FailingWriter, outgoing);
    assert_eq!(result.blocking_recv().unwrap(), Err("host output closed"));
}
