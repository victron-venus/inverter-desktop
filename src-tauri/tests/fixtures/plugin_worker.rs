//! Separate std-only executable used by the worker supervisor's pipe integration tests.
use std::io::{self, BufRead, Write};
use std::time::Duration;

fn field<'a>(line: &'a str, name: &str) -> &'a str {
    let key = format!("\"{name}\":\"");
    line.split_once(&key)
        .and_then(|(_, tail)| tail.split_once('"'))
        .map_or("", |(value, _)| value)
}

fn emit(value: &str) {
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{value}").unwrap();
    stdout.flush().unwrap();
}

fn contribute() {
    emit(
        r#"{"type":"contributions","items":[{"kind":"text","id":"greeting","title":"Fixture","text":"Separate executable"},{"kind":"action","id":"echo","title":"Echo","action_id":"echo","label":"Echo","params":{}},{"kind":"action","id":"hold","title":"Hold","action_id":"hold","label":"Hold","params":{}},{"kind":"action","id":"crash","title":"Crash","action_id":"crash","label":"Crash","params":{}},{"kind":"action","id":"cancel_count","title":"Cancellations","action_id":"cancel_count","label":"Count","params":{}}]}"#,
    );
}

fn notification(id: &str) {
    emit(&format!(
        "{{\"type\":\"notification\",\"id\":\"{id}\",\"title\":\"Private camera title\",\"body\":\"Private camera body\"}}"
    ));
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| {
        let executable = std::env::current_exe().unwrap();
        let stem = executable.file_stem().unwrap().to_string_lossy();
        if stem.starts_with("configuration") || stem.starts_with("notifications") {
            stem.into_owned()
        } else {
            "normal".into()
        }
    });
    let mut cancellations = 0;
    let mut configuration_revision = String::new();
    let mut configuration_secret_matches = false;
    let mut notification_sequence = 0;
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = line.unwrap();
        match field(&line, "type") {
            "hello" => {
                if mode == "no_hello" {
                    std::thread::sleep(Duration::from_secs(60));
                    return;
                }
                let id = if mode == "bad_identity" {
                    "other.worker"
                } else {
                    field(&line, "plugin_id")
                };
                let version = if mode == "bad_version" { 999 } else { 1 };
                let api = field(&line, "host_api_version");
                if mode == "notifications_before_ready" {
                    notification("premature");
                }
                emit(&format!("{{\"type\":\"ready\",\"protocol_version\":{version},\"host_api_version\":\"{api}\",\"plugin_id\":\"{id}\"}}"));
                if mode == "configuration_early_notification" {
                    notification("before-configuration-ack");
                }
                if mode == "oversize" {
                    print!("{}", "x".repeat(70_000));
                    io::stdout().flush().unwrap();
                    std::thread::sleep(Duration::from_secs(60));
                    return;
                }
                if !mode.starts_with("configuration") || mode == "configuration_early_data" {
                    contribute();
                }
                if mode == "notifications" {
                    notification("motion-1");
                    notification("motion-1");
                    notification("motion-2");
                }
                if mode == "notifications_oversize" {
                    emit(&format!("{{\"type\":\"notification\",\"id\":\"large\",\"title\":\"{}\",\"body\":\"Body\"}}", "x".repeat(129)));
                }
                if mode == "notifications_flood" {
                    for index in 0..200 {
                        notification(&format!("flood-{index}"));
                    }
                }
                if mode == "crash_always" {
                    std::process::exit(17);
                }
                if mode == "stalled" {
                    std::thread::sleep(Duration::from_secs(60));
                    return;
                }
                if mode == "flood" {
                    for _ in 0..200 {
                        emit(r#"{"type":"event","name":"tick","data":{}}"#);
                    }
                }
                if mode == "stderr_flood" {
                    let mut stderr = io::stderr().lock();
                    for _ in 0..128 {
                        stderr.write_all(&[b's'; 4096]).unwrap();
                    }
                    stderr.flush().unwrap();
                }
            }
            "configuration" => {
                configuration_revision = field(&line, "revision").to_owned();
                configuration_secret_matches = field(&line, "token") == "fixture-secret";
                if mode == "configuration_no_ack" {
                    std::thread::sleep(Duration::from_secs(60));
                    return;
                }
                if mode == "configuration_delayed" {
                    std::thread::sleep(Duration::from_millis(150));
                }
                let revision = if mode == "configuration_wrong_ack" {
                    "wrong"
                } else {
                    &configuration_revision
                };
                let ack =
                    format!("{{\"type\":\"configuration_ready\",\"revision\":\"{revision}\"}}");
                emit(&ack);
                if mode == "configuration_duplicate_ack" {
                    emit(&ack);
                }
                contribute();
                if mode == "configuration_notifications" {
                    notification("configured-motion");
                }
            }
            "action" => {
                let request_id = field(&line, "request_id");
                if field(&line, "action_id") == "echo" {
                    if mode == "notifications" {
                        notification("motion-1");
                        notification("motion-2");
                    }
                    if matches!(
                        mode.as_str(),
                        "notifications_actions" | "notifications_authorized"
                    ) {
                        for _ in 0..20 {
                            notification(&format!("motion-{notification_sequence}"));
                            notification_sequence += 1;
                        }
                    }
                }
                let value = match field(&line, "action_id") {
                    "hold" => continue,
                    "crash" => std::process::exit(23),
                    "cancel_count" => cancellations.to_string(),
                    _ => format!(
                        "{{\"ok\":true,\"pid\":{},\"inherited_environment\":{},\"configuration_revision\":\"{}\",\"configuration_secret_matches\":{}}}",
                        std::process::id(),
                        std::env::var_os("CARGO_MANIFEST_DIR").is_some(),
                        configuration_revision,
                        configuration_secret_matches
                    ),
                };
                emit(&format!("{{\"type\":\"action_result\",\"request_id\":\"{request_id}\",\"value\":{value}}}"));
            }
            "cancel" => {
                cancellations += 1;
                // A non-cooperative worker may send a result after cancellation.
                let request_id = field(&line, "request_id");
                emit(&format!("{{\"type\":\"action_result\",\"request_id\":\"{request_id}\",\"value\":\"late\"}}"));
            }
            "shutdown" => return,
            _ => {}
        }
    }
}
