#!/usr/bin/env node
/** Real production frontend with a synthetic Tauri IPC boundary; no native services. */
const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const http = require('node:http')
const { performance } = require('node:perf_hooks')
const { setTimeout: delay } = require('node:timers/promises')
const root = path.resolve(__dirname, '..')
const dist = path.join(root, 'dist')
const output = path.resolve(process.env.CI_BROWSER_DIAGNOSTICS || '.compatibility/local')
const evidence = path.resolve(process.env.CI_BROWSER_EVIDENCE || path.join(output, 'result.json'))
const engine = process.env.CI_BROWSER_ENGINE || 'chromium'
const channel = process.env.CI_BROWSER_CHANNEL || ''
assert(process.env.CI_PLAYWRIGHT_MODULE && path.isAbsolute(process.env.CI_PLAYWRIGHT_MODULE))
const playwright = require(process.env.CI_PLAYWRIGHT_MODULE)
assert(['chromium', 'firefox', 'webkit'].includes(engine))
assert(fs.existsSync(path.join(dist, 'index.html')), 'Build production frontend first')
fs.mkdirSync(output, { recursive: true })
fs.rmSync(evidence, { force: true })
// Use the pinned official Tauri mock, preserving callback/event semantics.
const mockFile = path.join(root, 'node_modules/@tauri-apps/api/mocks.js')
const mockModule = fs.readFileSync(mockFile, 'utf8')
assert(/export \{ clearMocks, mockConvertFileSrc, mockIPC, mockWindows \};/.test(mockModule))
const mocks = mockModule.replace(/export \{ clearMocks, mockConvertFileSrc, mockIPC, mockWindows \};/, '')
const fixture = {
  config: { mqtt_host: 'fixture.invalid', mqtt_port: 1883, mqtt_tls: false, setup_completed: true,
    color_scheme: 'dark', show_active_loads: true, show_batteries: true, show_solar_production: true,
    show_daily_stats: false, show_ev: false, show_home_section: false, show_header_toggles: false,
    desktop_plugins: [], modules: {} },
  state: { gt: 420, g1: 200, g2: 220, tt: 850, t1: 400, t2: 450, solar_total: 1200,
    battery_soc: 73, battery_power: -300, battery_voltage: 51.2, battery_current: -5.8,
    setpoint: 25, inverter_state: 'Inverting', uptime: 7200, dry_run: false,
    loads: { '1': 200, '2': -100 }, load_names: { '1': 'Fixture refrigerator', '2': 'Fixture solar' },
    batteries: [{ instance: 0, name: 'Fixture battery', soc: 73 }], features: {} },
}
const mime = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.png': 'image/png', '.svg': 'image/svg+xml', '.woff2': 'font/woff2', '.json': 'application/json', '.ico': 'image/x-icon' }
// The server only serves this fixed inventory; request URLs never touch the filesystem.
const staticAssets = new Map()
function loadStaticAssets(directory) {
  for (const item of fs.readdirSync(directory, { withFileTypes: true })) {
    const file = path.join(directory, item.name)
    assert(!item.isSymbolicLink(), 'Static assets must not contain links')
    if (item.isDirectory()) loadStaticAssets(file)
    else {
      assert(item.isFile(), 'Static assets must be regular files')
      const relative = path.relative(dist, file).split(path.sep).join('/')
      staticAssets.set(`/${relative}`, { data: fs.readFileSync(file), extension: path.extname(relative) })
    }
  }
}
loadStaticAssets(dist)
const server = http.createServer((req, res) => {
  const url = new URL(req.url, 'http://localhost')
  const route = ['/', '/about', '/config'].includes(url.pathname) ? '/index.html' : url.pathname
  const asset = staticAssets.get(route)
  if (!asset) return res.writeHead(404).end()
  res.setHeader('Content-Type', mime[asset.extension] || 'application/octet-stream')
  res.setHeader('Cache-Control', 'no-store')
  res.end(asset.data)
})
const scenarios = [], scenarioTimings = [], errors = [], blocked = [], consoleMessages = []
const timeouts = { actionMs: 30000, navigationMs: 15000, nativeStateMs: 10000 }
let failedScenario = null
async function eventually(read, predicate, message) {
  const deadline = performance.now() + timeouts.nativeStateMs
  let value
  while (performance.now() < deadline) { value = await read(); if (predicate(value)) return value; await delay(40) }
  assert.fail(`${message}: ${JSON.stringify(value)}`)
}
async function main() {
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve))
  const origin = `http://127.0.0.1:${server.address().port}`
  let browser, context, page
  try {
    const denied = await Promise.all(['/package.json', '/.git/config', '/%2e%2e%2fpackage.json'].map(async route => (await fetch(origin + route)).status))
    assert.deepEqual(denied, [404, 404, 404], 'Only inventoried distribution files may be served')
    browser = await playwright[engine].launch({ headless: true, ...(channel ? { channel } : {}) })
    context = await browser.newContext({ viewport: { width: 1280, height: 900 }, locale: 'en-US', serviceWorkers: 'block' })
    // Include driver input acknowledgement in the action budget. Assertions still
    // require the real application effect; timed-out actions are never retried.
    context.setDefaultTimeout(timeouts.actionMs)
    context.setDefaultNavigationTimeout(timeouts.navigationMs)
    await context.tracing.start({ screenshots: true, snapshots: true, sources: true })
    await context.route('**/*', route => {
      if (new URL(route.request().url()).origin === origin) return route.continue()
      blocked.push(route.request().url()); return route.abort()
    })
    await context.addInitScript({ content: `${mocks}\n(${function setup(fixture) {
      if (location.origin !== fixture.origin) return
      const f = window.__acceptance = { ...fixture, calls: [], unexpected: [], authFailure: true,
        unlocked: false, rejectSettings: false, notificationSession: null }
      localStorage.setItem('theme', f.config.color_scheme)
      mockWindows(location.pathname === '/about' ? 'about' : location.pathname === '/config' ? 'config' : 'main')
      f.emit = (event, payload) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', { event, payload })
      mockIPC(async (command, args = {}) => {
        // Native invoke serializes payloads; Vue reactive proxies are not cloneable.
        args = JSON.parse(JSON.stringify(args))
        f.calls.push({ command, args })
        switch (command) {
          case 'auth_status': if (f.authFailure) throw Error('Synthetic settings unavailable'); return { unlocked: f.unlocked }
          case 'auth_biometric_available': return false
          case 'auth_login':
            if (args.username !== 'fixture' || args.password !== 'synthetic-password') throw Error('Synthetic invalid login')
            f.unlocked = true; return 'synthetic-session'
          case 'get_config': return structuredClone(f.config)
          case 'save_config': f.config = structuredClone(args.config); return null
          case 'get_release_info': return { version: '9.8.7-rc.6', source_commit: '0'.repeat(40) }
          case 'get_state': return { ...f.state, notification_session: f.notificationSession }
          case 'connect_mqtt':
            f.notificationSession = args.notificationSession
            await f.emit('mqtt-connection-status', { connected: true, notification_session: f.notificationSession })
            return null
          case 'get_plugin_snapshot': case 'get_plugin_groups': return []
          case 'get_setpoint_override': return { value: null, last_error: null }
          case 'plugin:notification|is_permission_granted': return true
          case 'set_window_hidden': case 'disconnect_inverter': case 'send_notification':
          case 'test_mqtt_connection': case 'set_auto_start': case 'close_config_window':
          case 'plugin:window|close': case 'plugin:opener|open_url': return null
          case 'open_config_window': if (f.rejectSettings) throw Error('Synthetic window rejection'); return null
          case 'perform_action': return null
          default: f.unexpected.push(command); throw Error(`Unexpected native command ${command}`)
        }
      }, { shouldMockEvents: true })
    }.toString()})(${JSON.stringify({ ...fixture, origin })})` })
    context.on('page', page => { page.on('pageerror', error => errors.push(String(error))); page.on('console', message => consoleMessages.push({ type: message.type(), text: message.text() })) })
    page = await context.newPage()
    async function scenario(name, run) {
      const started = performance.now()
      let status = 'failed'
      try {
        await run()
        assert.deepEqual(errors, [])
        assert.deepEqual(await page.evaluate(() => window.__acceptance.unexpected), [])
        status = 'passed'
        scenarios.push(name); console.log(`PASS ${name}`)
      } catch (error) {
        failedScenario = name
        throw error
      } finally {
        scenarioTimings.push({ name, status, durationMs: Math.round((performance.now() - started) * 1000) / 1000 })
      }
    }
    const calls = () => page.evaluate(() => window.__acceptance.calls)
    const enable = () => page.evaluate(() => { window.__acceptance.authFailure = false })
    await scenario('native authentication failure keeps the dashboard closed', async () => {
      await page.goto(origin)
      await page.getByText('Unable to open settings: Error: Synthetic settings unavailable', { exact: true }).waitFor()
      assert.equal(await page.getByRole('region', { name: 'Inverter dashboard' }).count(), 0)
      assert(!(await calls()).some(call => call.command === 'connect_mqtt'))
    })
    await scenario('retry and real login form open the authenticated application', async () => {
      await enable(); await page.getByRole('button', { name: 'Retry', exact: true }).click()
      await page.getByRole('button', { name: 'Sign In', exact: true }).click()
      await page.getByText('Please enter username and password', { exact: true }).waitFor()
      await page.getByLabel('Username', { exact: true }).fill('fixture')
      await page.getByLabel('Password', { exact: true }).fill('synthetic-password')
      await page.getByRole('button', { name: 'Sign In', exact: true }).click()
      await page.getByRole('region', { name: 'Inverter dashboard' }).waitFor()
    })
    await scenario('production chart and telemetry render the native snapshot', async () => {
      await page.getByText('Fixture refrigerator', { exact: true }).waitFor()
      // Cached hydration is deliberately excluded from chart history; send live telemetry.
      await page.evaluate(async () => { const f = window.__acceptance; await f.emit('mqtt-state-update', { ...f.state, notification_session: f.notificationSession }) })
      await page.locator('canvas').first().waitFor()
      assert.match(await page.locator('body').innerText(), /73/)
      assert((await calls()).some(call => call.command === 'connect_mqtt' && call.args.host === 'fixture.invalid'))
    })
    await scenario('stale transport events are ignored and current telemetry updates', async () => {
      await page.evaluate(async () => { const f = window.__acceptance; await f.emit('mqtt-state-update', { ...f.state, load_names: { '1': 'Stale telemetry' }, notification_session: 'stale-session' }) })
      assert.equal(await page.getByText('Stale telemetry', { exact: true }).count(), 0)
      await page.evaluate(async () => { const f = window.__acceptance; await f.emit('mqtt-state-update', { ...f.state, load_names: { '1': 'Fresh telemetry' }, notification_session: f.notificationSession }) })
      await page.getByText('Fresh telemetry', { exact: true }).waitFor()
    })
    await scenario('dashboard control reaches typed native action', async () => {
      await page.getByRole('button', { name: 'DRY', exact: true }).click()
      await eventually(calls, list => list.some(c => c.command === 'perform_action' && c.args.action === 'dry_run' && c.args.payload.value === true), 'Dry run native payload')
    })
    await scenario('theme change persists through native configuration', async () => {
      await page.getByRole('button', { name: 'Light mode', exact: true }).click()
      await eventually(calls, list => list.some(c => c.command === 'save_config' && c.args.config.color_scheme === 'light'), 'Persisted light scheme')
      assert.equal(await page.evaluate(() => localStorage.getItem('theme')), 'light')
      assert.deepEqual(await page.evaluate(() => ({
        htmlDark: document.documentElement.classList.contains('dark'),
        bodyDark: document.body.classList.contains('dark'),
      })), { htmlDark: false, bodyDark: false })
    })
    await scenario('settings window failure is visible and retry remains possible', async () => {
      await page.evaluate(() => { window.__acceptance.rejectSettings = true })
      await page.getByRole('button', { name: 'Settings', exact: true }).click()
      await page.getByText('Failed to open config: Error: Synthetic window rejection', { exact: true }).waitFor()
      await page.evaluate(() => { window.__acceptance.rejectSettings = false })
      await page.getByRole('button', { name: 'Settings', exact: true }).click()
      assert.equal((await calls()).filter(c => c.command === 'open_config_window').length, 2)
    })
    await scenario('native session revocation removes protected content', async () => {
      await page.evaluate(async () => { const f = window.__acceptance; f.unlocked = false; await f.emit('auth-state-changed', {}) })
      await page.getByText('Authentication Required', { exact: true }).waitFor()
      assert.equal(await page.getByRole('region', { name: 'Inverter dashboard' }).count(), 0)
    })
    await scenario('configuration blocks plaintext credentials and saves explicit valid changes', async () => {
      await page.goto(`${origin}/config`); await enable()
      await page.evaluate(() => { window.__acceptance.unlocked = true })
      await page.getByRole('button', { name: 'Retry', exact: true }).click()
      await page.getByText('Configuration', { exact: true }).waitFor()
      await page.getByLabel('Username', { exact: true }).fill('fixture-user')
      await page.getByRole('button', { name: 'Save', exact: true }).click()
      await page.getByText('Enable TLS before using an MQTT username or password', { exact: true }).waitFor()
      assert(!(await calls()).some(c => c.command === 'save_config'))
      await page.getByLabel('Username', { exact: true }).fill('')
      await page.getByLabel('Host', { exact: true }).fill('updated-fixture.invalid')
      await page.getByRole('button', { name: 'Save', exact: true }).click()
      await page.getByText('Settings saved successfully', { exact: true }).waitFor()
      assert((await calls()).some(c => c.command === 'save_config' && c.args.config.mqtt_host === 'updated-fixture.invalid'))
      await page.getByRole('button', { name: 'Close settings', exact: true }).click()
      assert((await calls()).some(c => ['plugin:window|close', 'close_config_window'].includes(c.command)))
    })
    await scenario('about window uses native identity and safe opener commands', async () => {
      await page.goto(`${origin}/about`); await enable()
      await page.evaluate(() => { window.__acceptance.unlocked = true })
      await page.getByRole('button', { name: 'Retry', exact: true }).click()
      await page.getByText('Version 9.8.7-rc.6', { exact: true }).waitFor()
      await page.getByRole('button', { name: 'github.com/victron-venus/inverter-desktop', exact: true }).click()
      await page.getByRole('button', { name: 'Close', exact: true }).click()
      const native = await calls()
      assert(native.some(c => c.command === 'plugin:opener|open_url' && c.args.url === 'https://github.com/victron-venus/inverter-desktop'))
      assert(native.some(c => c.command === 'plugin:window|close'))
      assert.deepEqual(blocked, [])
      await page.screenshot({ path: path.join(output, 'about.png') })
    })
    const runtimeFile = path.join(output, 'runtime.json')
    const runtime = fs.existsSync(runtimeFile) ? JSON.parse(fs.readFileSync(runtimeFile)) : { sourceSha: 'local' }
    fs.writeFileSync(evidence, JSON.stringify({ ...runtime, engine: browser.browserType().name(), browserVersion: browser.version(), testsPassed: scenarios.length, testsFailed: 0, scenarios, scenarioTimings, timeouts, fixtureBoundary: 'synthetic native IPC using pinned official Tauri mocks' }, null, 2))
  } catch (error) {
    const native = page ? await page.evaluate(() => window.__acceptance).catch(() => null) : null
    fs.writeFileSync(path.join(output, 'failure.json'), JSON.stringify({ message: String(error.stack || error), failedScenario, scenarios, scenarioTimings, timeouts, errors, blocked, native, consoleMessages, chart: page ? await page.locator('.chart-wrap').evaluateAll(nodes => nodes.map(node => node.outerHTML)) : [] }, null, 2))
    if (page) await page.screenshot({ path: path.join(output, 'failure.png') }).catch(() => {})
    throw error
  } finally {
    if (context) { await context.tracing.stop({ path: path.join(output, 'trace.zip') }).catch(() => {}); await context.close() }
    if (browser) await browser.close()
    await new Promise(resolve => server.close(resolve))
  }
}
main().catch(error => { console.error(error); process.exitCode = 1 })
