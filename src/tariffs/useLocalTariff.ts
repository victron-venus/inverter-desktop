import { isTauri } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { tariffKey } from './storage'
import {
  loadLocalPreference,
  LOCAL_TARIFF_EVENT,
  localTariffEventScope,
  tariffModeKey,
  type TariffMode,
} from './localPreference'
import type { TariffPlan } from './model'

export function useLocalTariff(scope: () => string | null) {
  const mode = ref<TariffMode>('controller')
  const plan = ref<TariffPlan | null>(null)
  const error = ref('')
  const syncError = ref('')
  const ready = ref(!isTauri())
  let active = true
  let unlisten: UnlistenFn | undefined

  function load() {
    const current = scope()
    const value = current ? loadLocalPreference(current) : null
    mode.value = value?.mode ?? 'controller'
    plan.value = value?.plan ?? null
    error.value = value?.error ?? ''
  }
  function storageChanged(event: StorageEvent) {
    const current = scope()
    if (current && [null, tariffKey(current), tariffModeKey(current)].includes(event.key)) load()
  }
  function localChanged(event: Event) {
    if ((event as CustomEvent<string>).detail === scope()) load()
  }
  watch(scope, load, { immediate: true })
  onMounted(async () => {
    window.addEventListener('storage', storageChanged)
    window.addEventListener(LOCAL_TARIFF_EVENT, localChanged)
    if (!isTauri()) return
    try {
      const stop = await listen<unknown>(LOCAL_TARIFF_EVENT, ({ payload }) => {
        if (!active) return
        if (localTariffEventScope(payload) !== scope()) return
        // Never replay event values: a delayed notification must not undo a later save/clear.
        load()
      })
      if (!active) return stop()
      unlisten = stop
      ready.value = true
      load()
    } catch {
      if (active) syncError.value = 'Local tariff synchronization between windows is unavailable.'
    }
  })
  onBeforeUnmount(() => {
    active = false
    unlisten?.()
    window.removeEventListener('storage', storageChanged)
    window.removeEventListener(LOCAL_TARIFF_EVENT, localChanged)
  })
  return { mode, plan, error, syncError, ready }
}
