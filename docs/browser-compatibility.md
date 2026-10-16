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

The existing CI reusable workflow includes five required current-browser/OS
checks for pull requests, pushes and beta releases. Scheduled source audits,
manual audits without a release channel, RC runs and the existing final-build
stable release use the full 35-target
inventory: three pinned generations of Chromium, Firefox and WebKit on Linux,
Windows and macOS, plus installed Chrome/Edge channels. The release/quality gate
already requires the whole CI reusable workflow; browser failures remain failures.
The locks in `tools/browser-compatibility` isolate historical browser toolchains
from application dependencies.

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
