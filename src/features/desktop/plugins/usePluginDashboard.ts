import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { ref } from 'vue'
import type { ActionContribution, PluginSnapshot } from './types'

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
  let request = 0
  let listeners: Array<() => void> = []
  let startup: Promise<void> | undefined

  function clear() {
    plugins.value = []
    unavailable.value = false
    pendingActions.value = new Set()
    failedActions.value = new Set()
  }

  async function refresh() {
    if (!active || !authenticated) return
    const current = generation
    const revision = ++request
    try {
      const snapshot = await invoke<PluginSnapshot[]>('get_plugin_snapshot')
      if (!active || current !== generation || revision !== request) return
      plugins.value = snapshot
      unavailable.value = false
      const keys = new Set(
        snapshot.flatMap((plugin) =>
          plugin.contributions.flatMap((item) =>
            item.kind === 'action' ? [actionKey(plugin.plugin_id, item.action_id)] : []
          )
        )
      )
      failedActions.value = new Set([...failedActions.value].filter((key) => keys.has(key)))
    } catch {
      if (!active || current !== generation || revision !== request) return
      // Keep known cards readable but never let stale data authorize an action.
      unavailable.value = true
    }
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
    request += 1
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

  function actionKey(pluginId: string, actionId: string) {
    return JSON.stringify([pluginId, actionId])
  }

  function canAct(plugin: PluginSnapshot) {
    return active && authenticated && !unavailable.value && plugin.state === 'running'
  }

  async function runAction(pluginId: string, action: ActionContribution) {
    const plugin = plugins.value.find((entry) => entry.plugin_id === pluginId)
    const advertised = plugin?.contributions.find(
      (entry) =>
        entry.kind === 'action' && entry.id === action.id && entry.action_id === action.action_id
    )
    if (!plugin || !canAct(plugin) || advertised?.kind !== 'action') return
    const key = actionKey(pluginId, advertised.action_id)
    if (pendingActions.value.has(key)) return
    const current = generation
    pendingActions.value.add(key)
    failedActions.value.delete(key)
    try {
      await invoke('plugin_action', {
        pluginId,
        actionId: advertised.action_id,
        params: advertised.params,
      })
      if (active && current === generation) await refresh()
    } catch {
      if (active && current === generation) failedActions.value.add(key)
    } finally {
      if (active && current === generation) pendingActions.value.delete(key)
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
