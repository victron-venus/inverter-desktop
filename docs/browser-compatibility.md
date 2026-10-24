# Desktop browser compatibility

The desktop app owns its frontend: `src/main.ts` loads `App.vue`, `Config.vue`
or `About.vue`, and `src-tauri/tauri.conf.json` packages `dist`. It does not ship
the separately released inverter-dashboard-vue bundle. Its browser checks
therefore exercise this app's production build and Tauri command/event boundary.

`scripts/browser-acceptance.cjs` serves the real locked-dependency build locally.
It uses the pinned official `@tauri-apps/api/mocks` API for IPC callbacks, event
subscriptions and window identity. Only the native endpoint is synthetic:
no MQTT broker, inverter, credentials, OS notifications or native windows are
contacted. Unexpected native commands and external requests fail the check.

Ten scenarios cover authentication failure, retry and login, native telemetry and
chart rendering, stale transport events, typed control actions, persisted theme,
window-command rejection, session revocation, settings validation/save and About
identity/opener commands. Cached state is deliberately excluded from chart
history; the test sends a fresh native event before asserting chart rendering.
Configuration rejects plaintext MQTT credentials before dispatching save.

The existing CI reusable workflow selects the explicit
`windows-firefox-1.62.1` required profile for six browser/OS checks on pull
requests, pushes and beta releases: the five current-browser checks plus
Windows 2022 / Firefox 153 (Playwright 1.62.1). The latter retains coverage for a
historical browser that timed out while acknowledging a theme click even though
the application had persisted the new theme. Scheduled source audits, manual
audits without a release channel and RC runs use the full 35-target
inventory: three pinned generations of Chromium, Firefox and WebKit on Linux,
Windows and macOS, plus installed Chrome/Edge channels. The release/quality gate
already requires the whole CI reusable workflow; browser failures remain failures.
The locks in `tools/browser-compatibility` isolate historical browser toolchains
from application dependencies.

Main pushes pass the release planner's `beta` channel, producing a
source/run/attempt-bound browser verdict for automatically published betas.
Manual releases pass their selected channel. Scheduled source audits retain all
35 targets and individual evidence without a release verdict; the shared
`nightly` release profile uses the smaller baseline. Ordinary pull requests also
have no release channel.

These are source-built browser compatibility checks with a synthetic native
endpoint, not tests of every native OS webview or packaged release binary. The
existing Linux native acceptance, Apple TLS and Windows plugin checks remain
mandatory and cover the real Rust/native side separately.

For a local target, run `pnpm install --frozen-lockfile --ignore-scripts` and
`pnpm build`, install one isolated Playwright lock and its selected browser, then
set `CI_PLAYWRIGHT_MODULE` to the absolute `node_modules/playwright` directory,
`CI_BROWSER_ENGINE` to the engine, and run `node scripts/browser-acceptance.cjs`.
Optional `CI_BROWSER_CHANNEL`, `CI_BROWSER_DIAGNOSTICS` and `CI_BROWSER_EVIDENCE`
select the installed channel and evidence paths. CI retains browser identity,
scenario results, traces and failure screenshots for each target.

Browser actions have a bounded 30-second budget, including driver input
acknowledgement. Navigation remains bounded at 15 seconds and polling for a
native state change at 10 seconds. Failed actions are not retried or accepted
because their effect happened later. Theme acceptance also checks the rendered
HTML/body classes and persisted settings. Success and failure evidence includes
monotonic per-scenario durations; failure evidence names the failed scenario.
Together with the retained trace and application state, these timings help
diagnose driver delays and missing effects without relaxing the required
scenarios. Stable releases promote the verified RC bytes
and retain their original qualification evidence.
