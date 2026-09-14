//! Opt-in native media acceptance. This entry is absent from ordinary builds and
//! is never called by the production app, even when the feature is enabled.

use super::generation::{GenerationLease, RevokeOnDrop};
use super::media::MediaService;
use super::protocol::{HttpVideoGrant, PluginManifest, WorkerConfiguration};
use super::runtime::QueuedHttpVideo;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::Manager;
use tokio::time::{sleep, timeout};

const PLUGIN: &str = "native.media-smoke";
const STAGES: [&str; 3] = ["Smoke close", "Smoke revoke", "Smoke ended"];
const WAIT: Duration = Duration::from_secs(45);

struct Options {
    url: String,
    output: PathBuf,
    hold: Duration,
}

fn options(args: impl IntoIterator<Item = String>) -> Result<Options, String> {
    let mut args = args.into_iter();
    let mut url = None;
    let mut output = None;
    let mut hold = Duration::ZERO;
    while let Some(argument) = args.next() {
        let value = args.next().ok_or("Every option requires a value")?;
        match argument.as_str() {
            "--fixture-url" if url.is_none() => url = Some(value),
            "--evidence" if output.is_none() => output = Some(PathBuf::from(value)),
            "--hold-seconds" => {
                let seconds: u64 = value.parse().map_err(|_| "Invalid hold duration")?;
                if seconds > 30 {
                    return Err("Hold duration must be at most 30 seconds".into());
                }
                hold = Duration::from_secs(seconds);
            }
            _ => return Err("Unknown or repeated native media smoke option".into()),
        }
    }
    let result = Options {
        url: url.ok_or("--fixture-url is required (explicit loopback MP4)")?,
        output: output.ok_or("--evidence is required (new JSON file)")?,
        hold,
    };
    fixture_grant(&result.url)?;
    if !result.output.is_absolute() || result.output.file_name().is_none() {
        return Err("Evidence path must be an absolute new file path".into());
    }
    Ok(result)
}

fn fixture_grant(value: &str) -> Result<HttpVideoGrant, String> {
    let url = reqwest::Url::parse(value).map_err(|_| "Invalid fixture URL")?;
    let loopback = url
        .host_str()
        .and_then(|host| {
            host.trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .ok()
        })
        .is_some_and(|address| address.is_loopback());
    if !loopback || url.scheme() != "http" {
        return Err("Fixture must use plain HTTP at an explicit loopback IP".into());
    }
    let base = url.join(".").map_err(|_| "Invalid fixture base")?;
    let manifest: PluginManifest = serde_json::from_value(json!({
        "schema_version":1,"plugin_id":PLUGIN,"version":"0.0.0",
        "host_api":"^1.3","target":env!("INVERTER_DESKTOP_TARGET"),
        "entrypoint":"fixture", "permissions":["plugin_configuration","http_video"],
        "http_video":{"base_url_setting":"fixture_base"},
        "config_schema":{"type":"object","properties":{"fixture_base":{"type":"string"}}},
        "inventory":[],"signature":null
    }))
    .map_err(|_| "Cannot form native fixture declaration")?;
    let configuration = WorkerConfiguration {
        revision: "native-smoke".into(),
        values: json!({"fixture_base":base.as_str()}),
        secrets: BTreeMap::new(),
    };
    let grant = HttpVideoGrant::from_manifest_configuration(&manifest, Some(&configuration))?
        .ok_or("Fixture video grant unavailable")?;
    grant.validate_url(value)?;
    Ok(grant)
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Observation {
    kind: String,
    time: Option<f64>,
    duration: Option<f64>,
    width: u32,
    height: u32,
    ready_state: u8,
    error_code: Option<u8>,
}

#[derive(Serialize)]
struct ObservedVideo {
    stage: String,
    label: String,
    observation: Observation,
}

#[derive(Serialize)]
struct Evidence {
    scope: &'static str,
    passed: bool,
    failure: Option<String>,
    checks: BTreeMap<String, Value>,
    observations: Vec<ObservedVideo>,
}

struct SmokeState {
    evidence: Mutex<Evidence>,
    file: Mutex<File>,
    leases: Mutex<Vec<GenerationLease>>,
}

impl SmokeState {
    fn check(&self, key: &str, value: Value) {
        self.evidence
            .lock()
            .unwrap()
            .checks
            .insert(key.into(), value);
    }

    fn persist(&self) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(&*self.evidence.lock().unwrap())
            .map_err(|_| "Cannot encode native evidence")?;
        let mut file = self.file.lock().unwrap();
        file.seek(SeekFrom::Start(0))
            .and_then(|_| file.write_all(&bytes))
            .and_then(|_| file.set_len(bytes.len() as u64))
            .and_then(|_| file.sync_all())
            .map_err(|_| "Cannot persist native evidence".into())
    }

    fn revoke(&self) {
        for lease in self.leases.lock().unwrap().iter() {
            lease.revoke();
        }
    }

    fn saw(&self, stage: &str, kind: &str) -> bool {
        self.evidence
            .lock()
            .unwrap()
            .observations
            .iter()
            .any(|item| item.stage == stage && item.observation.kind == kind)
    }

    fn no_playback_error(&self) -> Result<(), String> {
        if self
            .evidence
            .lock()
            .unwrap()
            .observations
            .iter()
            .any(|item| item.observation.kind == "error")
        {
            Err("Native video decoder reported an error".into())
        } else {
            Ok(())
        }
    }
}

/// This harness command never calls the normal auth/config/keychain functions.
#[tauri::command]
fn auth_status() -> Value {
    json!({"enabled":false,"unlocked":true})
}

#[tauri::command]
fn observe_native_media_smoke(
    window: tauri::WebviewWindow,
    observation: Observation,
    state: tauri::State<'_, Arc<SmokeState>>,
) -> Result<(), String> {
    let stage = window.title().map_err(|_| "Cannot identify smoke window")?;
    if !super::media_windows::is_plugin_video_label(window.label())
        || !STAGES.contains(&stage.as_str())
        || !matches!(
            observation.kind.as_str(),
            "metadata" | "progress" | "ended" | "error"
        )
        || observation.width > 16384
        || observation.height > 16384
        || observation.ready_state > 4
        || observation
            .time
            .is_some_and(|n| !n.is_finite() || !(0.0..=600.0).contains(&n))
        || observation
            .duration
            .is_some_and(|n| !n.is_finite() || !(0.0..=600.0).contains(&n))
    {
        return Err("Invalid native smoke observation".into());
    }
    let mut evidence = state.evidence.lock().unwrap();
    if evidence.observations.len() >= 64 {
        return Err("Native smoke observation bound reached".into());
    }
    evidence.observations.push(ObservedVideo {
        stage,
        label: window.label().into(),
        observation,
    });
    Ok(())
}

// Observe native video events without replacing its source, playback, or controls.
const OBSERVER: &str = r#"(() => {
  if (window.__nativeSmokeObserver || !window.__TAURI_INTERNALS__) return;
  window.__nativeSmokeObserver = true;
  const sent = new Set();
  function report(video, kind) {
    if (!(video instanceof HTMLVideoElement) || sent.has(kind)) return;
    sent.add(kind);
    void window.__TAURI_INTERNALS__.invoke('observe_native_media_smoke', {observation: {
      kind, time: Number.isFinite(video.currentTime) ? video.currentTime : null,
      duration: Number.isFinite(video.duration) ? video.duration : null,
      width: video.videoWidth, height: video.videoHeight, ready_state: video.readyState,
      error_code: video.error ? video.error.code : null
    }}).catch(() => {});
  }
  function inspect(video) {
    if (!(video instanceof HTMLVideoElement)) return;
    if (video.readyState >= 1) report(video, 'metadata');
    if (video.currentTime >= 1.2 && video.readyState >= 2) report(video, 'progress');
    if (video.error) report(video, 'error');
    if (video.ended) report(video, 'ended');
  }
  for (const type of ['loadedmetadata', 'timeupdate', 'error', 'ended']) {
    document.addEventListener(type, event => {
      if (type === 'ended') report(event.target, 'ended');
      else inspect(event.target);
    }, true);
  }
  new MutationObserver(() => inspect(document.querySelector('video')))
    .observe(document, {subtree:true, childList:true});
  inspect(document.querySelector('video'));
})()"#;

async fn until<T>(mut read: impl FnMut() -> Result<Option<T>, String>) -> Result<T, String> {
    timeout(WAIT, async {
        loop {
            if let Some(value) = read()? {
                break Ok(value);
            }
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .map_err(|_| "Timed out waiting for real native media evidence")?
}

async fn playing_window(
    app: &tauri::AppHandle,
    state: &SmokeState,
    stage: &str,
) -> Result<tauri::WebviewWindow, String> {
    until(|| {
        state.no_playback_error()?;
        if !state.saw(stage, "metadata") || !state.saw(stage, "progress") {
            return Ok(None);
        }
        let decoded = state
            .evidence
            .lock()
            .unwrap()
            .observations
            .iter()
            .any(|item| {
                item.stage == stage
                    && item.observation.kind == "progress"
                    && item.observation.width > 0
                    && item.observation.height > 0
                    && item.observation.ready_state >= 2
                    && item.observation.time.is_some_and(|time| time >= 1.2)
                    && item
                        .observation
                        .duration
                        .is_some_and(|duration| duration >= 1.2)
                    && item.observation.error_code.is_none()
            });
        if !decoded {
            return Err(
                "Native progress has no decoded video dimensions or time advancement".into(),
            );
        }
        Ok(app.webview_windows().into_values().find(|window| {
            window.title().is_ok_and(|title| title == stage) && window.is_visible().unwrap_or(false)
        }))
    })
    .await
}

fn submit(
    media: &MediaService,
    state: &SmokeState,
    options: &Options,
    sequence: usize,
) -> Result<GenerationLease, String> {
    let lease = GenerationLease::new(PLUGIN.into(), 1, sequence as u64 + 1);
    state.leases.lock().unwrap().push(lease.clone());
    media
        .try_submit(QueuedHttpVideo {
            lease: lease.clone(),
            grant: fixture_grant(&options.url)?,
            id: format!("native-smoke-{sequence}"),
            url: options.url.clone(),
            title: STAGES[sequence].into(),
        })
        .map_err(|_| "Native media admission failed")?;
    Ok(lease)
}

fn geometry(window: &tauri::WebviewWindow) -> Result<Value, String> {
    let position = window
        .outer_position()
        .map_err(|_| "Window position unavailable")?;
    let size = window.outer_size().map_err(|_| "Window size unavailable")?;
    let monitor = window
        .current_monitor()
        .map_err(|_| "Monitor unavailable")?
        .ok_or("Window has no monitor")?;
    let work_area = monitor.work_area();
    let origin = &work_area.position;
    let bounds = &work_area.size;
    let contained = position.x >= origin.x
        && position.y >= origin.y
        && i64::from(position.x) + i64::from(size.width)
            <= i64::from(origin.x) + i64::from(bounds.width)
        && i64::from(position.y) + i64::from(size.height)
            <= i64::from(origin.y) + i64::from(bounds.height);
    if !contained || size.width == 0 || size.height == 0 {
        return Err("Native media window is outside its monitor bounds".into());
    }
    Ok(
        json!({"x":position.x,"y":position.y,"width":size.width,"height":size.height,
        "visible":window.is_visible().unwrap_or(false),"focused":window.is_focused().unwrap_or(false),
        "contained":contained}),
    )
}

async fn exercise(
    app: tauri::AppHandle,
    state: Arc<SmokeState>,
    profile: PathBuf,
    options: Options,
) -> Result<(), String> {
    let media = super::bridge::install_native_media_smoke(&app, profile.join("plugins")).await?;
    state.check("isolated_ready", json!(true));
    state.check("anchor_focused", json!(false));
    state.persist()?;
    let anchor = app
        .get_webview_window("smoke-anchor")
        .ok_or("Focus anchor missing")?;
    anchor
        .set_focus()
        .map_err(|_| "Cannot focus smoke anchor")?;
    until(|| {
        Ok(anchor
            .is_focused()
            .map_err(|_| "Anchor focus unavailable")?
            .then_some(()))
    })
    .await
    .map_err(|_| "Focus precondition failed: activate the Native Media Smoke anchor window before testing focus preservation")?;
    state.check("anchor_focused", json!(true));
    state.persist()?;

    let first_lease = submit(&media, &state, &options, 0)?;
    let _first_owner = RevokeOnDrop(first_lease.clone());
    let first = playing_window(&app, &state, STAGES[0]).await?;
    let second_lease = submit(&media, &state, &options, 1)?;
    let _second_owner = RevokeOnDrop(second_lease.clone());
    let second = playing_window(&app, &state, STAGES[1]).await?;
    let first_geometry = geometry(&first)?;
    let second_geometry = geometry(&second)?;
    let bounds = |value: &Value| {
        let x = value["x"].as_i64().expect("native x");
        let y = value["y"].as_i64().expect("native y");
        let width = value["width"].as_i64().expect("native width");
        let height = value["height"].as_i64().expect("native height");
        (x, y, x + width, y + height)
    };
    let (ax, ay, ar, ab) = bounds(&first_geometry);
    let (bx, by, br, bb) = bounds(&second_geometry);
    let separated = ar <= bx || br <= ax || ab <= by || bb <= ay;
    let focus_preserved = anchor.is_focused().unwrap_or(false)
        && !first.is_focused().unwrap_or(true)
        && !second.is_focused().unwrap_or(true);
    state.check(
        "two_native_windows",
        json!({"first":first_geometry,"second":second_geometry,
        "non_overlapping":separated,"anchor_focus_preserved":focus_preserved}),
    );
    if !separated || !focus_preserved {
        return Err("Native stacking or focus preservation failed".into());
    }
    let first_id = super::media_windows::media_id_for_label(first.label())
        .ok_or("Missing first media identity")?;
    let second_id = super::media_windows::media_id_for_label(second.label())
        .ok_or("Missing second media identity")?;
    let head = media
        .read_range(&first_id, first.label(), None, true)
        .await
        .map_err(|_| "Native media HEAD failed")?;
    let total = head.content_length;
    if total <= 1024 * 1024 {
        return Err("MP4 fixture must be larger than one MiB for range proof".into());
    }
    drop(head);
    let range = media
        .read_range(&first_id, first.label(), Some("bytes=0-"), false)
        .await
        .map_err(|_| "Native media range read failed")?;
    let bounded = range.status == 206
        && range.bytes.len() == 1024 * 1024
        && range.content_range == Some(format!("bytes 0-1048575/{total}"));
    state.check(
        "backend_range",
        json!({"status":range.status,"bytes":range.bytes.len(),
        "content_range":range.content_range,"whole_file_bytes":total,"bounded":bounded}),
    );
    drop(range);
    let wrong_window_denied = media
        .read_range(&first_id, second.label(), Some("bytes=0-1"), false)
        .await
        .is_err();
    state.check("backend_wrong_window_denied", json!(wrong_window_denied));
    if !bounded || !wrong_window_denied {
        return Err("Native range ownership check failed".into());
    }
    state.persist()?;
    sleep(options.hold).await;
    if app.get_webview_window(first.label()).is_none()
        || app.get_webview_window(second.label()).is_none()
    {
        return Err("Fixture ended before the requested observation hold completed".into());
    }
    // Exercise the production Vue button and its owned-close IPC, not destroy().
    first
        .eval("document.querySelector('button[aria-label=\"Close\"]')?.click()")
        .map_err(|_| "Cannot activate real video close control")?;
    until(|| {
        Ok(app
            .get_webview_window(first.label())
            .is_none()
            .then_some(()))
    })
    .await?;
    state.check("real_close_control_destroyed_window", json!(true));
    if media.is_window_active(&first_id, first.label()) {
        return Err("User-closed media remains accessible".into());
    }
    second_lease.revoke();
    let revoked = !media.is_window_active(&second_id, second.label());
    state.check("revoked_access_before_native_close", json!(revoked));
    if !revoked {
        return Err("Revoked media remained accessible".into());
    }
    until(|| {
        Ok(app
            .get_webview_window(second.label())
            .is_none()
            .then_some(()))
    })
    .await?;
    until(|| Ok((!media.has_owned_work()).then_some(()))).await?;
    state.check("revocation_destroyed_window", json!(true));

    let final_lease = submit(&media, &state, &options, 2)?;
    let _final_owner = RevokeOnDrop(final_lease);
    let final_window = playing_window(&app, &state, STAGES[2]).await?;
    until(|| {
        state.no_playback_error()?;
        Ok(state.saw(STAGES[2], "ended").then_some(()))
    })
    .await?;
    until(|| {
        Ok(app
            .get_webview_window(final_window.label())
            .is_none()
            .then_some(()))
    })
    .await?;
    until(|| Ok((!media.has_owned_work()).then_some(()))).await?;
    state.check("ended_event_and_automatic_close", json!(true));
    let empty = std::fs::read_dir(profile.join("desktop-plugin-media"))
        .map_err(|_| "Cannot inspect owned media cleanup")?
        .next()
        .is_none();
    state.check("owned_media_directory_empty", json!(empty));
    if !empty {
        return Err("Owned media files remain after window cleanup".into());
    }
    Ok(())
}

/// Launch the isolated native harness from its required-feature example only.
/// Arguments: --fixture-url http://127.0.0.1:PORT/prefix/clip.mp4
/// --evidence /absolute/new-evidence.json [--hold-seconds 0..30].
pub fn run() -> Result<(), String> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments == ["--help"] {
        println!("plugin-media-smoke --fixture-url http://127.0.0.1:PORT/prefix/clip.mp4 --evidence /absolute/new-evidence.json [--hold-seconds 0..30]");
        println!("Use a decodable video MP4 larger than one MiB, lasting 8–40 seconds (at least hold + 6 seconds). No production services or settings are opened.");
        return Ok(());
    }
    let options = options(arguments)?;
    let mut open = OpenOptions::new();
    open.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        open.mode(0o600);
    }
    let output = open
        .open(&options.output)
        .map_err(|_| "Evidence file must be new and writable")?;
    let profile = tempfile::Builder::new()
        .prefix("inverter-native-media-smoke-")
        .tempdir()
        .map_err(|_| "Cannot create private smoke profile")?;
    let state = Arc::new(SmokeState {
        evidence: Mutex::new(Evidence { scope:"Native fixture lease; actual media service, route, Vue player and OS windows. Signed worker/MQTT are proved separately.",
            passed:false,failure:None,checks:BTreeMap::new(),observations:Vec::new() }),
        file: Mutex::new(output), leases:Mutex::new(Vec::new()),
    });
    let mut context = tauri::generate_context!();
    context.config_mut().identifier = format!(
        "com.alvit.native-media-smoke.{}",
        uuid::Uuid::new_v4().simple()
    );
    context.config_mut().product_name = Some("Native Media Smoke".into());
    context.config_mut().app.windows.clear();
    context.config_mut().build.dev_url = None;
    context.config_mut().app.security.asset_protocol.enable = false;
    context.package_info_mut().name = "Native Media Smoke".into();
    let setup_state = state.clone();
    let root = profile
        .path()
        .canonicalize()
        .map_err(|_| "Cannot canonicalize private smoke profile")?;
    let builder = super::media_windows::register(tauri::Builder::default())
        .manage(state.clone())
        .invoke_handler(tauri::generate_handler![
            auth_status,
            observe_native_media_smoke,
            super::bridge::close_plugin_video_window,
            super::bridge::drag_plugin_video_window
        ])
        .on_page_load(|webview, _| {
            if super::media_windows::is_plugin_video_label(webview.label()) {
                let _ = webview.eval(OBSERVER);
            }
        })
        .setup(move |app| {
            tauri::WebviewWindowBuilder::new(
                app,
                "smoke-anchor",
                tauri::WebviewUrl::External("about:blank".parse()?),
            )
            .incognito(true)
            .title("Native media smoke — focus anchor")
            .inner_size(460.0, 140.0)
            .focused(true)
            .build()?;
            let app = app.handle().clone();
            let run_state = setup_state.clone();
            tauri::async_runtime::spawn(async move {
                let result = timeout(
                    Duration::from_secs(180),
                    exercise(app.clone(), run_state.clone(), root, options),
                )
                .await
                .map_err(|_| "Native smoke overall deadline expired".to_owned())
                .and_then(|result| result);
                run_state.revoke();
                let failed = result.is_err();
                {
                    let mut evidence = run_state.evidence.lock().unwrap();
                    // A completed scenario is not a completed native shutdown.
                    evidence
                        .checks
                        .insert("scenario_completed".into(), json!(!failed));
                    evidence.failure = result.err();
                }
                let _ = run_state.persist();
                app.exit(i32::from(failed));
            });
            Ok(())
        });
    let app = match builder.build(context) {
        Ok(app) => app,
        Err(_) => {
            state.evidence.lock().unwrap().failure =
                Some("Cannot build isolated native application".into());
            state.persist()?;
            return Err("Cannot build isolated native application".into());
        }
    };
    let exit_state = state.clone();
    let exit_code = app.run_return(move |app, event| {
        if matches!(
            &event,
            tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
        ) {
            exit_state.revoke();
        }
        super::bridge::on_run_event(app, event);
    });
    state.check("native_exit_code", json!(exit_code));
    let profile_closed = profile.close().is_ok();
    state.check("private_profile_removed", json!(profile_closed));
    {
        let mut evidence = state.evidence.lock().unwrap();
        evidence.passed = exit_code == 0
            && profile_closed
            && evidence.checks.get("scenario_completed") == Some(&json!(true))
            && evidence.failure.is_none();
        if !evidence.passed && evidence.failure.is_none() {
            evidence.failure =
                Some("Native exit or private profile cleanup did not complete successfully".into());
        }
    }
    state.persist()?;
    if !state.evidence.lock().unwrap().passed {
        return Err("Native smoke did not pass; inspect its structured evidence".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixture_input_is_explicit_loopback_without_url_ambiguity() {
        assert!(fixture_grant("http://127.0.0.1:9000/prefix/clip.mp4").is_ok());
        for value in [
            "http://example.com/clip.mp4",
            "http://localhost/clip.mp4",
            "http://127.0.0.1/clip.mp4?secret=value",
            "http://user@127.0.0.1/clip.mp4",
            "http://127.0.0.1/prefix/%2e%2e/clip.mp4",
        ] {
            assert!(fixture_grant(value).is_err());
        }
        assert!(options(Vec::new()).is_err());
        let relative = options(
            [
                "--fixture-url",
                "http://127.0.0.1:9000/clip.mp4",
                "--evidence",
                "relative.json",
            ]
            .map(str::to_owned),
        );
        assert_eq!(
            relative.err().as_deref(),
            Some("Evidence path must be an absolute new file path")
        );
        let absolute = std::env::temp_dir().join("native-smoke-options-only.json");
        assert!(options(vec![
            "--fixture-url".into(),
            "http://127.0.0.1:9000/clip.mp4".into(),
            "--evidence".into(),
            absolute.to_string_lossy().into_owned(),
        ])
        .is_ok());
    }
}
