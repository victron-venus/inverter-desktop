import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createI18n } from 'vue-i18n'
import Config from '../../../Config.vue'
import { defaultConfig } from '../../../config'
import en from '../messages.en'
import ru from '../messages.ru'
import PluginManager from './PluginManager.vue'
import PluginSettingsEditor from './PluginSettingsEditor.vue'
import RetainedPluginData from './RetainedPluginData.vue'
import { createPluginManager } from './usePluginManager'
import type {
  ManagedPlugin,
  PluginManagerSnapshot,
  PluginPackagePreview,
  PluginSettingsSaveResult,
  PluginSettingsView,
  RetainedPluginDataSnapshot,
} from './types'

const native = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn(), emit: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: native.invoke }))
vi.mock('@tauri-apps/api/event', () => ({ listen: native.listen, emit: native.emit }))
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ close: vi.fn() }) }))

const installed: ManagedPlugin = {
  plugin_id: 'example.monitor',
  version: '2.0.0',
  rollback_version: '1.0.0',
  enabled: true,
  permissions: ['dashboard_contributions'],
  error: null,
  runtime: {
    plugin_id: 'example.monitor',
    state: 'running',
    generation: 1,
    restart_count: 0,
    contributions: [],
    last_error: null,
  },
}
const selected: PluginPackagePreview = {
  token: 'native-preview-token',
  plugin_id: 'example.monitor',
  version: '2.0.0',
  current_version: null,
  permissions: ['dashboard_contributions', 'network_http'],
  target: 'aarch64-apple-darwin',
  host_api: '^1.0.0',
  publisher_key_id: 'approved-key',
  restart_required: false,
}
let snapshot: PluginManagerSnapshot
let selection: PluginPackagePreview | null
let unlocked: boolean
let wrapper: VueWrapper | undefined
const controllers: Array<ReturnType<typeof createPluginManager>> = []
const callbacks = new Map<string, () => void>()
const stops: Array<ReturnType<typeof vi.fn>> = []
const handlers = new Map<string, () => unknown>()

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (value: unknown) => void
  const promise = new Promise<T>((done, fail) => {
    resolve = done
    reject = fail
  })
  return { promise, resolve, reject }
}

function event(name: string) {
  const callback = callbacks.get(name)
  if (!callback) throw new Error(`Missing listener: ${name}`)
  callback()
}

function controller() {
  const value = createPluginManager(() => 'Operation failed')
  controllers.push(value)
  return value
}

function managerWrapper() {
  if (!wrapper) throw new Error('Manager is not mounted')
  return wrapper
}

function activeEditorKey(value: ReturnType<typeof createPluginManager>) {
  if (!value.settingsEditor.value) throw new Error('Settings editor is not open')
  return value.settingsEditor.value.key
}

function translations(locale = 'en') {
  return createI18n({
    legacy: false,
    locale,
    fallbackLocale: 'en',
    messages: { en, ru },
    missingWarn: false,
    fallbackWarn: false,
  })
}

async function openManager(locale = 'en') {
  wrapper = mount(PluginManager, { global: { plugins: [translations(locale)] } })
  await flushPromises()
}

function button(label: string) {
  const found = wrapper?.findAll('button').find((value) => value.text() === label)
  if (!found) throw new Error(`Missing button: ${label}`)
  return found
}

function calls(command: string) {
  return native.invoke.mock.calls.filter(([name]) => name === command)
}

beforeEach(() => {
  snapshot = {
    ready: true,
    error: null,
    installation_available: true,
    target: selected.target,
    data_revision: '0',
    plugins: [],
  }
  selection = structuredClone(selected)
  unlocked = true
  handlers.clear()
  native.invoke.mockReset().mockImplementation(async (command: string) => {
    if (handlers.has(command)) return handlers.get(command)?.()
    switch (command) {
      case 'auth_status':
        return { unlocked }
      case 'get_plugin_manager_snapshot':
        return structuredClone(snapshot)
      case 'preview_plugin_package':
        return structuredClone(selection)
      case 'get_config':
        return { ...defaultConfig, mqtt_host: 'cerbo', setup_completed: true }
      case 'get_state':
        return {}
      default:
        return undefined
    }
  })
  native.emit.mockReset().mockResolvedValue(undefined)
  native.listen.mockReset().mockImplementation(async (name: string, callback: () => void) => {
    callbacks.set(name, callback)
    const stop = vi.fn(() => {
      if (callbacks.get(name) === callback) callbacks.delete(name)
    })
    stops.push(stop)
    return stop
  })
})

afterEach(() => {
  wrapper?.unmount()
  wrapper = undefined
  for (const value of controllers) value.stop()
  controllers.length = 0
  callbacks.clear()
  stops.length = 0
})

describe('desktop plugin manager', () => {
  it.each([
    ['en', 'Desktop notifications'],
    ['ru', 'Уведомления на компьютере'],
  ])('labels notification capability before installation in %s', async (locale, label) => {
    selection = { ...selected, permissions: ['desktop_notifications'] }
    await openManager(locale)
    await button(locale === 'en' ? 'Choose plugin package…' : 'Выбрать пакет плагина…').trigger(
      'click'
    )
    await flushPromises()
    expect(wrapper?.text()).toContain(label)
    expect(wrapper?.text()).not.toContain('desktop_notifications')
    expect(calls('install_plugin_package')).toHaveLength(0)
  })

  it('shows the actual empty publisher policy and does not offer installation', async () => {
    snapshot.installation_available = false
    await openManager()
    expect(wrapper?.text()).toContain('This build has no approved plugin publishers.')
    expect(wrapper?.text()).toContain('No plugins installed.')
    expect(wrapper?.text()).toContain('Home Assistant and cameras are still included')
    expect(button('Choose plugin package…').attributes('disabled')).toBeDefined()
    await button('Choose plugin package…').trigger('click')
    expect(calls('preview_plugin_package')).toHaveLength(0)
  })

  it('shows escaped verified metadata and capabilities before an explicit installation', async () => {
    selection = {
      ...selected,
      plugin_id: '<img src=x onerror=alert(1)>',
      publisher_key_id: '<b>publisher</b>',
    }
    await openManager()
    await button('Choose plugin package…').trigger('click')
    await flushPromises()
    expect(native.invoke).toHaveBeenCalledWith('preview_plugin_package')
    expect(wrapper?.text()).toContain('<img src=x onerror=alert(1)>')
    expect(wrapper?.find('img').exists()).toBe(false)
    expect(wrapper?.find('b').exists()).toBe(false)
    expect(wrapper?.text()).toContain('Dashboard panels and actions')
    expect(wrapper?.text()).toContain('HTTP network connections')
    expect(wrapper?.text()).not.toContain(selected.token)
    expect(calls('install_plugin_package')).toHaveLength(0)
    await wrapper?.find('input[type="checkbox"]').setValue(false)
    snapshot.plugins = [{ ...installed, enabled: false }]
    await button('Install').trigger('click')
    await flushPromises()
    expect(native.invoke).toHaveBeenCalledWith('install_plugin_package', {
      token: selected.token,
      enable: false,
    })
    expect(wrapper?.text()).toContain('Disabled')
    expect(wrapper?.text()).not.toContain('Review package')
  })

  it('reviews the installed and proposed versions before updating and cancels previews explicitly', async () => {
    selection = { ...selected, current_version: '1.0.0' }
    await openManager()
    await button('Choose plugin package…').trigger('click')
    await flushPromises()
    expect(wrapper?.text()).toContain('Installed version')
    expect(wrapper?.text()).toContain('1.0.0')
    expect(button('Update').exists()).toBe(true)
    await button('Cancel').trigger('click')
    await flushPromises()
    expect(native.invoke).toHaveBeenCalledWith('discard_plugin_package', { token: selected.token })
    expect(calls('install_plugin_package')).toHaveLength(0)
    expect(wrapper?.text()).not.toContain('Review package')
  })

  it('treats native picker cancellation as a no-op', async () => {
    selection = null
    await openManager()
    await button('Choose plugin package…').trigger('click')
    await flushPromises()
    expect(wrapper?.find('[role="alert"]').exists()).toBe(false)
    expect(wrapper?.text()).not.toContain('Review package')
    expect(calls('install_plugin_package')).toHaveLength(0)
  })

  it('shows enable, disable, rollback destination and concrete uninstall confirmation', async () => {
    snapshot.plugins = [structuredClone(installed)]
    await openManager()
    expect(wrapper?.text()).toContain('Running')
    snapshot.plugins[0].enabled = false
    await button('Disable').trigger('click')
    await flushPromises()
    expect(native.invoke).toHaveBeenCalledWith('set_plugin_enabled', {
      pluginId: installed.plugin_id,
      enabled: false,
    })
    expect(wrapper?.text()).toContain('Disabled')
    snapshot.plugins[0].enabled = true
    await button('Enable').trigger('click')
    await flushPromises()
    expect(native.invoke).toHaveBeenCalledWith('set_plugin_enabled', {
      pluginId: installed.plugin_id,
      enabled: true,
    })
    await button('Roll back to 1.0.0').trigger('click')
    await flushPromises()
    expect(native.invoke).toHaveBeenCalledWith('rollback_plugin_package', {
      pluginId: installed.plugin_id,
    })
    await button('Uninstall…').trigger('click')
    expect(wrapper?.text()).toContain('Uninstall example.monitor version 2.0.0?')
    expect(calls('uninstall_plugin_package')).toHaveLength(0)
    await button('Cancel').trigger('click')
    expect(calls('uninstall_plugin_package')).toHaveLength(0)
    await button('Uninstall…').trigger('click')
    snapshot.plugins = []
    await button('Uninstall plugin').trigger('click')
    await flushPromises()
    expect(native.invoke).toHaveBeenCalledWith('uninstall_plugin_package', {
      pluginId: installed.plugin_id,
      deleteSettings: false,
    })
    expect(wrapper?.text()).toContain('No plugins installed.')
  })

  it('prevents duplicate operations and requires a fresh preview after an expired token', async () => {
    const pending = deferred<void>()
    handlers.set('install_plugin_package', () => pending.promise)
    const value = controller()
    await value.start()
    await value.pickPackage()
    const first = value.install()
    await value.install()
    await value.pickPackage()
    expect(calls('install_plugin_package')).toHaveLength(1)
    expect(calls('preview_plugin_package')).toHaveLength(1)
    expect(value.busy.value).toBe(true)
    pending.reject('Preview expired')
    await first
    expect(value.preview.value).toBeNull()
    expect(value.error.value).toBe('Preview expired')
    expect(value.installFailed.value).toBe(true)
    expect(value.busy.value).toBe(false)
    await value.install()
    expect(calls('install_plugin_package')).toHaveLength(1)
  })

  it('renders bounded escaped errors and recovers by refreshing', async () => {
    handlers.set('preview_plugin_package', () => {
      throw new Error('<script>failure</script>')
    })
    await openManager()
    await button('Choose plugin package…').trigger('click')
    await flushPromises()
    expect(wrapper?.find('[role="alert"]').text()).toBe('<script>failure</script>')
    expect(wrapper?.find('script').exists()).toBe(false)
    await button('Refresh').trigger('click')
    await flushPromises()
    expect(wrapper?.find('[role="alert"]').exists()).toBe(false)
  })

  it('disables mutations while manager initialization or snapshot retrieval has failed', async () => {
    snapshot.ready = false
    snapshot.error = 'Plugin storage unavailable'
    await openManager()
    expect(button('Choose plugin package…').attributes('disabled')).toBeDefined()
    expect(wrapper?.text()).toContain('Plugin storage unavailable')
    snapshot.ready = true
    snapshot.error = null
    await button('Refresh').trigger('click')
    await flushPromises()
    expect(button('Choose plugin package…').attributes('disabled')).toBeUndefined()
    handlers.set('get_plugin_manager_snapshot', () => {
      throw new Error('Refresh failed')
    })
    event('plugin-host-update')
    await flushPromises()
    expect(button('Choose plugin package…').attributes('disabled')).toBeDefined()
  })

  it('clears previews and installed data immediately on logout and discards late picker results', async () => {
    snapshot.plugins = [structuredClone(installed)]
    const pending = deferred<PluginPackagePreview | null>()
    handlers.set('preview_plugin_package', () => pending.promise)
    const value = controller()
    await value.start()
    const picking = value.pickPackage()
    unlocked = false
    event('auth-state-changed')
    expect(value.snapshot.value).toBeNull()
    expect(value.preview.value).toBeNull()
    pending.resolve(structuredClone(selected))
    await picking
    await flushPromises()
    expect(value.preview.value).toBeNull()
    expect(value.snapshot.value).toBeNull()
    expect(native.invoke).toHaveBeenCalledWith('discard_plugin_package', { token: selected.token })
    unlocked = true
    event('auth-state-changed')
    await flushPromises()
    expect(value.snapshot.value?.plugins).toHaveLength(1)
    expect(native.listen).toHaveBeenCalledTimes(2)
  })

  it('ignores outdated snapshot and authentication responses', async () => {
    snapshot.plugins = [structuredClone(installed)]
    const value = controller()
    await value.start()
    const oldSnapshot = deferred<PluginManagerSnapshot>()
    handlers.set('get_plugin_manager_snapshot', () => oldSnapshot.promise)
    const refreshing = value.refresh()
    handlers.delete('get_plugin_manager_snapshot')
    snapshot.plugins = []
    event('plugin-host-update')
    await flushPromises()
    oldSnapshot.resolve({ ...snapshot, plugins: [structuredClone(installed)] })
    await refreshing
    expect(value.snapshot.value?.plugins).toEqual([])
    const oldAuth = deferred<{ unlocked: boolean }>()
    handlers.set('auth_status', () => oldAuth.promise)
    event('auth-state-changed')
    handlers.delete('auth_status')
    unlocked = false
    event('auth-state-changed')
    await flushPromises()
    oldAuth.resolve({ unlocked: true })
    await flushPromises()
    expect(value.snapshot.value).toBeNull()
  })

  it('coalesces a burst of host updates into one in-flight query and one fresh follow-up', async () => {
    const value = controller()
    await value.start()
    const pending = deferred<PluginManagerSnapshot>()
    handlers.set('get_plugin_manager_snapshot', () => pending.promise)
    const first = value.refresh()
    const beforeBurst = calls('get_plugin_manager_snapshot').length
    for (let index = 0; index < 100; index += 1) event('plugin-host-update')
    expect(calls('get_plugin_manager_snapshot')).toHaveLength(beforeBurst)
    handlers.delete('get_plugin_manager_snapshot')
    snapshot.plugins = [structuredClone(installed)]
    pending.resolve({ ...snapshot, plugins: [] })
    await first
    expect(calls('get_plugin_manager_snapshot')).toHaveLength(beforeBurst + 1)
    expect(value.snapshot.value?.plugins).toHaveLength(1)
  })

  it('disables all mutation controls while busy and drops errors completed after logout', async () => {
    snapshot.plugins = [structuredClone(installed)]
    const pending = deferred<void>()
    handlers.set('set_plugin_enabled', () => pending.promise)
    await openManager()
    await button('Disable').trigger('click')
    expect(button('Choose plugin package…').attributes('disabled')).toBeDefined()
    expect(button('Roll back to 1.0.0').attributes('disabled')).toBeDefined()
    expect(button('Uninstall…').attributes('disabled')).toBeDefined()
    unlocked = false
    event('auth-state-changed')
    await flushPromises()
    expect(wrapper?.text()).not.toContain(installed.plugin_id)
    pending.reject(new Error('stale worker failure'))
    await flushPromises()
    expect(wrapper?.text()).not.toContain('stale worker failure')
    expect(calls('save_config')).toHaveLength(0)
    expect(calls('connect_mqtt')).toHaveLength(0)
  })

  it('releases late picker tokens after unmount without restoring the preview', async () => {
    const pending = deferred<PluginPackagePreview | null>()
    handlers.set('preview_plugin_package', () => pending.promise)
    const value = controller()
    await value.start()
    const picking = value.pickPackage()
    value.stop()
    pending.resolve(structuredClone(selected))
    await picking
    await flushPromises()
    expect(value.preview.value).toBeNull()
    expect(value.snapshot.value).toBeNull()
    expect(native.invoke).toHaveBeenCalledWith('discard_plugin_package', { token: selected.token })
  })

  it('does not carry pending subscriptions across a controller restart', async () => {
    const pending = deferred<() => void>()
    const lateStop = vi.fn()
    native.listen.mockReturnValueOnce(pending.promise)
    const value = controller()
    const first = value.start()
    value.stop()
    await Promise.all([value.start(), value.start()])
    pending.resolve(lateStop)
    await first
    expect(lateStop).toHaveBeenCalledOnce()
    expect(native.listen).toHaveBeenCalledTimes(3)
    expect(value.snapshot.value?.ready).toBe(true)
    event('plugin-host-update')
    await flushPromises()
    expect(value.snapshot.value?.ready).toBe(true)
  })

  it('cleans up preview tokens and listeners when the settings panel is removed', async () => {
    await openManager()
    await button('Choose plugin package…').trigger('click')
    await flushPromises()
    wrapper?.unmount()
    wrapper = undefined
    await flushPromises()
    expect(native.invoke).toHaveBeenCalledWith('discard_plugin_package', { token: selected.token })
    expect(stops.every((stop) => stop.mock.calls.length === 1)).toBe(true)
    expect(callbacks.size).toBe(0)
  })

  it('cleans up a late listener and fails closed if subscription setup fails', async () => {
    const pending = deferred<() => void>()
    const stop = vi.fn()
    native.listen.mockReturnValueOnce(pending.promise)
    const value = controller()
    const starting = value.start()
    value.stop()
    pending.resolve(stop)
    await starting
    expect(stop).toHaveBeenCalledOnce()
    expect(calls('get_plugin_manager_snapshot')).toHaveLength(0)
    native.listen.mockRejectedValueOnce(new Error('Subscription failed'))
    await value.start()
    expect(value.error.value).toBe('Subscription failed')
    expect(value.snapshot.value).toBeNull()
    await value.retry()
    expect(value.snapshot.value?.ready).toBe(true)
  })

  it('mounts management only after opening the desktop settings tab and translates it', async () => {
    wrapper = mount(Config, { global: { plugins: [translations('ru')] } })
    await flushPromises()
    expect(calls('get_plugin_manager_snapshot')).toHaveLength(0)
    await button('Плагины').trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('Плагины не установлены.')
    expect(calls('get_plugin_manager_snapshot')).toHaveLength(1)
    await button('MQTT Broker').trigger('click')
    await flushPromises()
    expect(wrapper.findComponent(PluginManager).exists()).toBe(false)
  })

  it('retains settings by default and requests deletion only after explicit selection', async () => {
    snapshot.plugins = [structuredClone(installed)]
    await openManager()
    await button('Uninstall…').trigger('click')
    const checkbox = () => managerWrapper().find('input[name="delete-plugin-settings"]')
    expect((checkbox().element as HTMLInputElement).checked).toBe(false)
    await checkbox().setValue(true)
    await button('Cancel').trigger('click')
    await button('Uninstall…').trigger('click')
    expect((checkbox().element as HTMLInputElement).checked).toBe(false)
    await checkbox().setValue(true)
    await button('Uninstall plugin').trigger('click')
    await flushPromises()
    expect(native.invoke).toHaveBeenCalledWith('uninstall_plugin_package', {
      pluginId: installed.plugin_id,
      deleteSettings: true,
    })
  })

  it('shows settings only for configured permission and blocks lifecycle actions during a settings read', async () => {
    snapshot.plugins = [structuredClone(installed)]
    await openManager()
    expect(wrapper?.findAll('button').some((item) => item.text() === 'Settings')).toBe(false)
    snapshot.plugins[0].permissions.push('plugin_configuration')
    event('plugin-host-update')
    await flushPromises()
    const pending = deferred<PluginSettingsView>()
    handlers.set('get_plugin_settings', () => pending.promise)
    await button('Settings').trigger('click')
    await flushPromises()
    expect(button('Disable').attributes('disabled')).toBeDefined()
    expect(button('Uninstall…').attributes('disabled')).toBeDefined()
    expect(button('Choose plugin package…').attributes('disabled')).toBeDefined()
    await button('Disable').trigger('click')
    expect(calls('set_plugin_enabled')).toHaveLength(0)
    snapshot.plugins[0].version = '3.0.0'
    event('plugin-host-update')
    await flushPromises()
    expect(wrapper?.findComponent(PluginSettingsEditor).exists()).toBe(false)
    pending.resolve(settingsView())
    await flushPromises()
    expect(wrapper?.findComponent(PluginSettingsEditor).exists()).toBe(false)
  })

  it('clears settings on logout and drops save completion from the old editor', async () => {
    snapshot.plugins = [{ ...structuredClone(installed), permissions: ['plugin_configuration'] }]
    handlers.set('get_plugin_settings', settingsView)
    const pending = deferred<PluginSettingsSaveResult>()
    handlers.set('save_plugin_settings', () => pending.promise)
    await openManager()
    await button('Settings').trigger('click')
    await flushPromises()
    await managerWrapper().find('input[name="token"]').setValue('temporary-secret')
    await managerWrapper().find('form').trigger('submit')
    expect(button('Roll back to 1.0.0').attributes('disabled')).toBeDefined()
    unlocked = false
    event('auth-state-changed')
    await flushPromises()
    expect(wrapper?.findComponent(PluginSettingsEditor).exists()).toBe(false)
    const queries = calls('get_plugin_manager_snapshot').length
    pending.resolve({ settings: settingsView(), restart_error: 'old failure' })
    await flushPromises()
    expect(wrapper?.text()).not.toContain('old failure')
    expect(wrapper?.text()).not.toContain('Settings saved.')
    expect(calls('get_plugin_manager_snapshot')).toHaveLength(queries)
    expect(calls('save_config')).toHaveLength(0)
    expect(calls('connect_mqtt')).toHaveLength(0)
  })

  it('closes idle settings before mutations and ignores busy callbacks from replaced editors', async () => {
    snapshot.plugins = [{ ...structuredClone(installed), permissions: ['plugin_configuration'] }]
    const value = controller()
    await value.start()
    value.openSettings(installed.plugin_id)
    const firstKey = activeEditorKey(value)
    value.setSettingsBusy(firstKey, false)
    value.openSettings(installed.plugin_id)
    const currentKey = activeEditorKey(value)
    value.setSettingsBusy(firstKey, false)
    expect(value.canManage.value).toBe(false)
    value.setSettingsBusy(currentKey, false)
    await value.setEnabled(installed.plugin_id, false)
    expect(value.settingsEditor.value).toBeNull()
    expect(native.invoke).toHaveBeenCalledWith('set_plugin_enabled', {
      pluginId: installed.plugin_id,
      enabled: false,
    })
  })

  it('does not scan retained data on startup or worker updates and scans on explicit open', async () => {
    handlers.set('get_retained_plugin_data', retainedView)
    await openManager()
    for (let index = 0; index < 100; index += 1) event('plugin-host-update')
    await flushPromises()
    expect(calls('get_retained_plugin_data')).toHaveLength(0)
    await button('Stored plugin data').trigger('click')
    await flushPromises()
    expect(wrapper?.findComponent(RetainedPluginData).exists()).toBe(true)
    expect(calls('get_retained_plugin_data')).toHaveLength(1)
    for (let index = 0; index < 100; index += 1) event('plugin-host-update')
    await flushPromises()
    expect(calls('get_retained_plugin_data')).toHaveLength(1)
  })

  it('locks package and editor actions during retained deletion and clears the panel on logout', async () => {
    snapshot.plugins = [{ ...structuredClone(installed), permissions: ['plugin_configuration'] }]
    handlers.set('get_retained_plugin_data', retainedView)
    const pending = deferred<void>()
    handlers.set('delete_retained_plugin_data', () => pending.promise)
    await openManager()
    await button('Stored plugin data').trigger('click')
    await flushPromises()
    await button('Delete stored data…').trigger('click')
    await button('Permanently delete data').trigger('click')
    expect(button('Choose plugin package…').attributes('disabled')).toBeDefined()
    expect(button('Settings').attributes('disabled')).toBeDefined()
    expect(button('Uninstall…').attributes('disabled')).toBeDefined()
    unlocked = false
    event('auth-state-changed')
    await flushPromises()
    expect(wrapper?.findComponent(RetainedPluginData).exists()).toBe(false)
    pending.reject(new Error('stale deletion error'))
    await flushPromises()
    expect(wrapper?.text()).not.toContain('stale deletion error')
    expect(calls('get_retained_plugin_data')).toHaveLength(1)
  })

  it('refreshes retained ownership after uninstall without scanning for unrelated worker changes', async () => {
    snapshot.plugins = [structuredClone(installed)]
    let data = retainedView()
    data.records[0].plugin_id = installed.plugin_id
    handlers.set('get_retained_plugin_data', () => structuredClone(data))
    handlers.set('uninstall_plugin_package', () => {
      snapshot.plugins = []
      snapshot.data_revision = '1'
      data = retainedView()
    })
    await openManager()
    await button('Stored plugin data').trigger('click')
    await flushPromises()
    expect(button('Delete stored data…').attributes('disabled')).toBeDefined()
    await button('Uninstall…').trigger('click')
    await button('Uninstall plugin').trigger('click')
    await flushPromises()
    expect(calls('get_retained_plugin_data')).toHaveLength(2)
    expect(wrapper?.text()).toContain('Unidentified stored data')
    expect(button('Delete stored data…').attributes('disabled')).toBeUndefined()
  })

  it.each([true, false])(
    'restores an open inventory after setting enabled=%s advances the native data revision',
    async (enabled) => {
      snapshot.plugins = [{ ...structuredClone(installed), enabled: !enabled }]
      handlers.set('get_retained_plugin_data', retainedView)
      handlers.set('set_plugin_enabled', () => {
        snapshot.plugins[0].enabled = enabled
        snapshot.data_revision = '1'
      })
      await openManager()
      await button('Stored plugin data').trigger('click')
      await flushPromises()
      expect(calls('get_retained_plugin_data')).toHaveLength(1)
      await button(enabled ? 'Enable' : 'Disable').trigger('click')
      await flushPromises()
      expect(calls('get_retained_plugin_data')).toHaveLength(2)
      expect(wrapper?.text()).toContain('Unidentified stored data')
      expect(button('Delete stored data…').attributes('disabled')).toBeUndefined()
      expect(button('Stored plugin data').attributes('disabled')).toBeUndefined()
      event('plugin-host-update')
      await flushPromises()
      expect(calls('get_retained_plugin_data')).toHaveLength(2)
    }
  )

  it('invalidates stale deletion consent when a package becomes installed elsewhere', async () => {
    const data = retainedView()
    handlers.set('get_retained_plugin_data', () => structuredClone(data))
    await openManager()
    await button('Stored plugin data').trigger('click')
    await flushPromises()
    await button('Delete stored data…').trigger('click')
    snapshot.plugins = [structuredClone(installed)]
    data.records[0].plugin_id = installed.plugin_id
    snapshot.data_revision = '1'
    event('plugin-host-update')
    await flushPromises()
    expect(wrapper?.text()).not.toContain('Its settings and secrets cannot be recovered.')
    expect(button('Delete stored data…').attributes('disabled')).toBeDefined()
    expect(calls('get_retained_plugin_data')).toHaveLength(2)
    expect(calls('delete_retained_plugin_data')).toHaveLength(0)
  })

  it('keeps settings editing and retained data separate and reloads only on the next explicit open', async () => {
    snapshot.plugins = [{ ...structuredClone(installed), permissions: ['plugin_configuration'] }]
    handlers.set('get_retained_plugin_data', retainedView)
    handlers.set('get_plugin_settings', settingsView)
    handlers.set('save_plugin_settings', () => ({ settings: settingsView(), restart_error: null }))
    await openManager()
    await button('Stored plugin data').trigger('click')
    await flushPromises()
    await button('Settings').trigger('click')
    await flushPromises()
    expect(wrapper?.findComponent(RetainedPluginData).exists()).toBe(false)
    await managerWrapper().find('form').trigger('submit')
    await flushPromises()
    expect(calls('get_retained_plugin_data')).toHaveLength(1)
    await button('Stored plugin data').trigger('click')
    await flushPromises()
    expect(wrapper?.findComponent(PluginSettingsEditor).exists()).toBe(false)
    expect(calls('get_retained_plugin_data')).toHaveLength(2)
  })

  it('invalidates orphan deletion consent after data changes in another window without package changes', async () => {
    let data = retainedView()
    handlers.set('get_retained_plugin_data', () => structuredClone(data))
    await openManager()
    await button('Stored plugin data').trigger('click')
    await flushPromises()
    await button('Delete stored data…').trigger('click')
    expect(wrapper?.text()).toContain('Its settings and secrets cannot be recovered.')
    data = { ...data, records: [], total_bytes: 0 }
    snapshot.data_revision = '1'
    event('plugin-host-update')
    await flushPromises()
    expect(wrapper?.text()).toContain('No stored data records.')
    expect(wrapper?.text()).not.toContain('Its settings and secrets cannot be recovered.')
    expect(calls('get_retained_plugin_data')).toHaveLength(2)
    expect(calls('delete_retained_plugin_data')).toHaveLength(0)
    for (let index = 0; index < 100; index += 1) event('plugin-host-update')
    await flushPromises()
    expect(calls('get_retained_plugin_data')).toHaveLength(2)
  })

  it('refreshes once when successful deletion is broadcast before its IPC promise resolves', async () => {
    let data = retainedView()
    const pending = deferred<void>()
    handlers.set('get_retained_plugin_data', () => structuredClone(data))
    handlers.set('delete_retained_plugin_data', () => pending.promise)
    await openManager()
    await button('Stored plugin data').trigger('click')
    await flushPromises()
    await button('Delete stored data…').trigger('click')
    await button('Permanently delete data').trigger('click')
    data = { ...data, records: [], total_bytes: 0 }
    snapshot.data_revision = '1'
    event('plugin-host-update')
    await flushPromises()
    expect(calls('get_retained_plugin_data')).toHaveLength(1)
    expect(button('Stored plugin data').attributes('disabled')).toBeDefined()
    pending.resolve()
    await flushPromises()
    expect(calls('get_retained_plugin_data')).toHaveLength(2)
    expect(wrapper?.text()).toContain('No stored data records.')
    expect(button('Stored plugin data').attributes('disabled')).toBeUndefined()
    expect(wrapper?.find('[role="alert"]').exists()).toBe(false)
  })
})

function retainedView(): RetainedPluginDataSnapshot {
  return {
    records: [{ record_id: 'a'.repeat(64), revision: 'b'.repeat(64), bytes: 128, plugin_id: null }],
    total_bytes: 128,
    max_records: 128,
    max_bytes: 8_388_608,
  }
}

function settingsView(): PluginSettingsView {
  return {
    plugin_id: installed.plugin_id,
    version: installed.version,
    revision: 'settings-revision',
    fields: [
      {
        key: 'token',
        title: 'Token',
        description: null,
        type: 'string',
        required: false,
        secret: true,
        enum: null,
        minimum: null,
        maximum: null,
        min_length: null,
        max_length: null,
      },
    ],
    values: {},
    secret_present: { token: true },
  }
}
