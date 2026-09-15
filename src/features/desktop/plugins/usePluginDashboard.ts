import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { ref } from 'vue'
import type { ActionContribution, PluginSnapshot } from './types'

/** Native-validated JSON objects compare by content, independent of key order. */
function canonical(value: unknown): string {
  return JSON.stringify(value, (_key, item) => {
    if (item === null || typeof item !== 'object' || Array.isArray(item)) return item
    return Object.fromEntries(
      Object.keys(item)
        .sort((left, right) => {
          // Distinct Unicode spellings remain distinct JSON keys.
          if (left < right) return -1
          if (left > right) return 1
          return 0
        })
        .map((key) => [key, item[key]])
    )
  })
}

function actionKey(pluginId: string, instanceId: string | null, action: ActionContribution) {
  // Display-only changes must not unlock a pending operation or hide its error.
  return canonical([pluginId, instanceId, action.action_id, action.params])
}

/** Each mounted dashboard owns its subscriptions; the native host owns worker processes. */
export function createPluginDashboard() {
  const plugins = ref<PluginSnapshot[]>([])
  const unavailable = ref(false)
  const pendingActions = ref(new Set<string>())
  const failedActions = ref(new Set<string>())
  let active = false
  let authenticated = false
  let generation = 0
  let lifecycle = 0
  let refreshRequested = false
  let refreshing: Promise<void> | undefined
  const operations = new Map<string, { pluginId: string; instanceId: string }>()
  let listeners: Array<() => void> = []
  let startup: Promise<void> | undefined

  function clear() {
    plugins.value = []
    unavailable.value = false
    pendingActions.value = new Set()
    failedActions.value = new Set()
    operations.clear()
    refreshRequested = false
  }

  function current(session: number) {
    return active && authenticated && session === generation
  }

  async function readSnapshot(session: number) {
    try {
      const snapshot = await invoke<PluginSnapshot[]>('get_plugin_snapshot')
      if (!current(session)) return
      plugins.value = snapshot
      unavailable.value = false
      // A temporarily withdrawn action can return after a network reconnect.
      // Preserve its pending/unknown outcome until the actual worker is replaced.
      for (const [key, operation] of operations) {
        const running = snapshot.some(
          (plugin) =>
            plugin.plugin_id === operation.pluginId &&
            plugin.instance_id === operation.instanceId &&
            plugin.state === 'running'
        )
        if (!running) {
          operations.delete(key)
          failedActions.value.delete(key)
          pendingActions.value.delete(key)
        }
      }
    } catch {
      if (!current(session)) return
      // Keep known cards readable but never let stale data authorize an action.
      unavailable.value = true
    }
  }

  async function drainRefreshes() {
    try {
      while (refreshRequested && active && authenticated) {
        refreshRequested = false
        // Apply every completed snapshot even when another update is waiting.
        await readSnapshot(generation)
      }
    } finally {
      refreshing = undefined
    }
  }

  function refresh(): Promise<void> {
    if (!active || !authenticated) return Promise.resolve()
    refreshRequested = true
    refreshing ??= drainRefreshes()
    return refreshing
  }

  async function refreshSession() {
    if (!active) return
    const current = ++generation
    authenticated = false
    clear()
    try {
      const status = await invoke<{ unlocked: boolean }>('auth_status')
      if (!active || current !== generation) return
      authenticated = status.unlocked === true
      await refresh()
    } catch {
      // AuthGate presents authentication failures. Contributions remain hidden.
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
    authenticated = false
    lifecycle += 1
    generation += 1
    for (const unlisten of listeners) unlisten()
    listeners = []
    startup = undefined
    clear()
  }

  function start(): Promise<void> {
    if (startup) return startup
    if (active) return Promise.resolve()
    active = true
    const lifetime = ++lifecycle
    startup = (async () => {
      try {
        await subscribe('auth-state-changed', () => void refreshSession(), lifetime)
        if (!active || lifetime !== lifecycle) return
        await subscribe('plugin-host-update', () => void refresh(), lifetime)
        if (active && lifetime === lifecycle) await refreshSession()
      } catch {
        if (active && lifetime === lifecycle) stop()
      } finally {
        if (lifetime === lifecycle) startup = undefined
      }
    })()
    return startup
  }

  function canAct(plugin: PluginSnapshot) {
    return (
      active &&
      authenticated &&
      !unavailable.value &&
      !!plugin.instance_id &&
      plugin.state === 'running'
    )
  }

  async function runAction(
    pluginId: string,
    instanceId: string | null,
    action: ActionContribution
  ) {
    const plugin = plugins.value.find((entry) => entry.plugin_id === pluginId)
    if (!plugin || !instanceId || plugin.instance_id !== instanceId || !canAct(plugin)) return
    const descriptor = canonical(action)
    const advertised = plugin.contributions.some(
      (entry) => entry.kind === 'action' && canonical(entry) === descriptor
    )
    if (!advertised) return
    const key = actionKey(pluginId, instanceId, action)
    if (pendingActions.value.has(key)) return
    const session = generation
    const operation = { pluginId, instanceId }
    operations.set(key, operation)
    pendingActions.value.add(key)
    failedActions.value.delete(key)
    try {
      await invoke('plugin_action', {
        pluginId,
        instanceId,
        actionId: action.action_id,
        params: action.params,
      })
    } catch {
      if (current(session) && operations.get(key) === operation) failedActions.value.add(key)
    } finally {
      if (current(session) && operations.get(key) === operation) {
        pendingActions.value.delete(key)
        if (!failedActions.value.has(key)) operations.delete(key)
        // Recheck the state after either outcome without holding busy behind
        // a continuous stream of refreshes or retrying the service operation.
        void refresh()
      }
    }
  }

  return {
    plugins,
    unavailable,
    pendingActions,
    failedActions,
    start,
    stop,
    refresh,
    canAct,
    actionKey,
    runAction,
  }
}
