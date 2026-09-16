import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { computed, ref } from 'vue'
import type { PluginManagerSnapshot, PluginPackagePreview } from './types'
import { createRetainedPluginData } from './useRetainedPluginData'

async function discardToken(token: string) {
  try {
    await invoke('discard_plugin_package', { token })
  } catch {
    // Revocation also expires native previews; teardown must remain best effort.
  }
}

/** The config window reviews native-owned previews; no filesystem paths cross IPC. */
export function createPluginManager(fallbackError: () => string) {
  const snapshot = ref<PluginManagerSnapshot | null>(null)
  const preview = ref<PluginPackagePreview | null>(null)
  const enableAfterInstall = ref(true)
  const confirmRemoval = ref<string | null>(null)
  const deleteSettings = ref(false)
  const settingsEditor = ref<{ plugin_id: string; version: string; key: number } | null>(null)
  const settingsBusy = ref(false)
  const busy = ref(false)
  const loading = ref(true)
  const error = ref<string | null>(null)
  const installFailed = ref(false)
  const authorized = ref(false)
  const connected = ref(false)
  let active = false
  let generation = 0
  let lifecycle = 0
  let refreshRequested = false
  let refreshing: Promise<void> | undefined
  let startup: Promise<void> | undefined
  let listeners: Array<() => void> = []
  let editorKey = 0
  const retainedData = createRetainedPluginData(
    fallbackError,
    () =>
      !!(
        active &&
        authorized.value &&
        connected.value &&
        snapshot.value?.ready &&
        !busy.value &&
        !settingsBusy.value
      )
  )
  const working = computed(() => busy.value || settingsBusy.value || retainedData.busy.value)
  const hasCurrentSnapshot = computed(
    () => authorized.value && active && connected.value && snapshot.value?.ready === true
  )
  const canManage = computed(() => hasCurrentSnapshot.value && !working.value)
  const canInstall = computed(() => canManage.value && snapshot.value?.installation_available)

  function message(value: unknown) {
    let text = ''
    if (value instanceof Error) text = value.message
    else if (typeof value === 'string') text = value
    return text.slice(0, 512) || fallbackError()
  }

  function clearPreview() {
    const previous = preview.value
    preview.value = null
    if (previous) void discardToken(previous.token)
  }

  function clearSettings() {
    settingsEditor.value = null
    settingsBusy.value = false
  }

  function clear() {
    clearPreview()
    clearSettings()
    retainedData.close()
    snapshot.value = null
    confirmRemoval.value = null
    deleteSettings.value = false
    authorized.value = false
    connected.value = false
    busy.value = false
    error.value = null
    installFailed.value = false
    enableAfterInstall.value = true
    refreshRequested = false
  }

  function current(session: number) {
    return active && session === generation
  }

  async function readSnapshot(session: number) {
    try {
      const value = await invoke<PluginManagerSnapshot>('get_plugin_manager_snapshot')
      if (!current(session)) return
      const previous = snapshot.value
      snapshot.value = value
      connected.value = true
      if (!value.plugins.some((plugin) => plugin.plugin_id === confirmRemoval.value)) {
        confirmRemoval.value = null
        deleteSettings.value = false
      }
      const editor = settingsEditor.value
      if (
        editor &&
        (!value.ready ||
          !value.plugins.some(
            (plugin) =>
              plugin.plugin_id === editor.plugin_id &&
              plugin.version === editor.version &&
              plugin.permissions.includes('plugin_configuration')
          ))
      )
        clearSettings()
      if (!value.ready) retainedData.close()
      else if (previous && previous.data_revision !== value.data_revision) {
        // Native package/data changes invalidate consent, including changes made
        // in another window. Worker-only updates keep this revision unchanged.
        retainedData.invalidate()
        void retainedData.refresh()
      }
    } catch (error_) {
      if (!current(session)) return
      connected.value = false
      clearSettings()
      retainedData.close()
      error.value = message(error_)
    } finally {
      if (current(session)) loading.value = false
    }
  }

  async function drainRefreshes() {
    try {
      while (refreshRequested && active && authorized.value) {
        refreshRequested = false
        await readSnapshot(generation)
      }
    } finally {
      // Clear before settling the promise so a later event cannot be stranded
      // behind an already completed drain. Auth changes keep this in-flight guard.
      refreshing = undefined
    }
  }

  function refresh(): Promise<void> {
    if (!active || !authorized.value) return Promise.resolve()
    refreshRequested = true
    refreshing ??= drainRefreshes()
    return refreshing
  }

  async function refreshSession() {
    if (!active) return
    const session = ++generation
    clear()
    loading.value = true
    try {
      const status = await invoke<{ unlocked: boolean }>('auth_status')
      if (!current(session)) return
      authorized.value = status.unlocked === true
      await refresh()
    } catch (error_) {
      if (current(session)) error.value = message(error_)
    } finally {
      if (current(session)) loading.value = false
    }
  }

  async function subscribe(name: string, handler: () => void, lifetime: number) {
    const stop = await listen(name, () => {
      if (active && lifetime === lifecycle) handler()
    })
    if (active && lifetime === lifecycle) listeners.push(stop)
    else stop()
  }

  function stop() {
    active = false
    generation += 1
    lifecycle += 1
    for (const unlisten of listeners) unlisten()
    listeners = []
    startup = undefined
    clear()
    loading.value = false
  }

  function start(): Promise<void> {
    if (startup) return startup
    if (active) return Promise.resolve()
    active = true
    loading.value = true
    const lifetime = ++lifecycle
    startup = (async () => {
      try {
        await subscribe('auth-state-changed', () => void refreshSession(), lifetime)
        if (!active || lifetime !== lifecycle) return
        await subscribe('plugin-host-update', () => void refresh(), lifetime)
        if (active && lifetime === lifecycle) await refreshSession()
      } catch (error_) {
        if (active && lifetime === lifecycle) {
          stop()
          error.value = message(error_)
        }
      } finally {
        if (lifetime === lifecycle) startup = undefined
      }
    })()
    return startup
  }

  async function retry() {
    if (working.value) return
    if (active) await refreshSession()
    else await start()
  }

  async function pickPackage() {
    if (!canInstall.value) return
    clearSettings()
    retainedData.close()
    const session = generation
    busy.value = true
    error.value = null
    installFailed.value = false
    confirmRemoval.value = null
    const previous = preview.value
    preview.value = null
    try {
      if (previous) await discardToken(previous.token)
      if (!current(session)) return
      const selected = await invoke<PluginPackagePreview | null>('preview_plugin_package')
      if (!current(session)) {
        if (selected) void discardToken(selected.token)
        return
      }
      preview.value = selected
      enableAfterInstall.value = true
    } catch (error_) {
      if (current(session)) error.value = message(error_)
    } finally {
      if (current(session)) busy.value = false
    }
  }

  async function mutate(command: string, args: Record<string, unknown>, installing = false) {
    if (!canManage.value) return
    clearSettings()
    retainedData.invalidate()
    const session = generation
    busy.value = true
    error.value = null
    installFailed.value = false
    confirmRemoval.value = null
    deleteSettings.value = false
    if (!installing) clearPreview()
    try {
      await invoke(command, args)
      if (current(session)) await refresh()
    } catch (error_) {
      if (current(session)) {
        // Native may have persisted a lifecycle change before cleanup failed.
        // Refresh ownership while retaining the original operation error.
        await refresh()
        if (current(session)) {
          error.value = message(error_)
          installFailed.value = installing
        }
      }
    } finally {
      if (current(session)) {
        busy.value = false
        // Every package operation can advance native data_revision, including
        // enable/disable. Resume an open inventory after releasing the busy guard.
        await retainedData.refresh()
      }
    }
  }

  async function install() {
    if (!canInstall.value || !preview.value) return
    const token = preview.value.token
    preview.value = null
    await mutate('install_plugin_package', { token, enable: enableAfterInstall.value }, true)
  }

  function findPlugin(pluginId: string) {
    return snapshot.value?.plugins.find((plugin) => plugin.plugin_id === pluginId)
  }

  async function setEnabled(pluginId: string, enabled: boolean) {
    if (findPlugin(pluginId)) await mutate('set_plugin_enabled', { pluginId, enabled })
  }

  async function rollback(pluginId: string) {
    if (findPlugin(pluginId)?.rollback_version && !findPlugin(pluginId)?.configuration_managed)
      await mutate('rollback_plugin_package', { pluginId })
  }

  function requestRemoval(pluginId: string) {
    if (!canManage.value || !findPlugin(pluginId)) return
    clearSettings()
    retainedData.cancelDeletion()
    confirmRemoval.value = pluginId
    deleteSettings.value = false
  }

  async function uninstall(pluginId: string) {
    if (confirmRemoval.value === pluginId && findPlugin(pluginId)) {
      await mutate('uninstall_plugin_package', { pluginId, deleteSettings: deleteSettings.value })
    }
  }

  function openSettings(pluginId: string) {
    const plugin = findPlugin(pluginId)
    if (!canManage.value || !plugin?.permissions.includes('plugin_configuration')) return
    retainedData.close()
    clearPreview()
    confirmRemoval.value = null
    settingsEditor.value = { plugin_id: pluginId, version: plugin.version, key: ++editorKey }
    settingsBusy.value = true
  }

  function setSettingsBusy(key: number, value: boolean) {
    if (settingsEditor.value?.key === key) settingsBusy.value = value
  }

  function closeSettings(key: number) {
    if (settingsEditor.value?.key === key) clearSettings()
  }

  async function settingsSaved(key: number) {
    if (settingsEditor.value?.key === key) {
      retainedData.invalidate()
      await refresh()
    }
  }

  async function openRetainedData() {
    if (!canManage.value) return
    clearSettings()
    clearPreview()
    confirmRemoval.value = null
    await retainedData.open()
  }

  return {
    snapshot,
    preview,
    enableAfterInstall,
    confirmRemoval,
    deleteSettings,
    settingsEditor,
    retainedData,
    retainedDataOpened: retainedData.opened,
    working,
    busy,
    loading,
    error,
    installFailed,
    hasCurrentSnapshot,
    canManage,
    canInstall,
    start,
    stop,
    retry,
    retryConfigured: () => mutate('retry_configured_plugins', {}),
    refresh,
    pickPackage,
    clearPreview,
    install,
    setEnabled,
    rollback,
    requestRemoval,
    uninstall,
    openSettings,
    setSettingsBusy,
    closeSettings,
    settingsSaved,
    openRetainedData,
  }
}
