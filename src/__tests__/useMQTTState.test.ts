import { describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import { applyInverterState, mqttConnected, state } from '../composables/useInverterState'
import { useMQTTState } from '../composables/useMQTTState'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => vi.fn()(...args),
}))

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
  emit: vi.fn(),
}))

vi.mock('vue-i18n', () => ({
  useI18n: () => ({ t: (key: string) => key }),
}))

describe('evSectionVisible latch', () => {
  it('latches true once ev_present is seen and never clears', async () => {
    // shallowRef(state): mutate via applyInverterState (object replace), matching MQTT path
    applyInverterState({ ev_present: false, evcharger_present: false })
    mqttConnected.value = true
    const { evSectionVisible, evLatch } = useMQTTState()
    evLatch.value = false
    expect(evSectionVisible.value).toBe(false)

    applyInverterState({ ev_present: true })
    await nextTick()
    expect(evSectionVisible.value).toBe(true)

    applyInverterState({ ev_present: false })
    mqttConnected.value = false
    await nextTick()
    expect(evSectionVisible.value).toBe(true)
  })

  it('latches from evcharger_present', async () => {
    applyInverterState({ ev_present: false, evcharger_present: false })
    mqttConnected.value = true
    const { evSectionVisible, evLatch } = useMQTTState()
    evLatch.value = false
    expect(evSectionVisible.value).toBe(false)

    applyInverterState({ evcharger_present: true })
    await nextTick()
    expect(evSectionVisible.value).toBe(true)
  })

  it('stays hidden when no presence was ever seen', async () => {
    // Reset presence bits by replacing state object (undefined fields stay absent
    // after merge if previously set — clear explicitly with a fresh base).
    state.value = { booleans: {}, features: {}, ui_config: {} }
    mqttConnected.value = true
    const { evSectionVisible, evLatch } = useMQTTState()
    evLatch.value = false
    expect(evSectionVisible.value).toBe(false)

    mqttConnected.value = false
    applyInverterState({ ev_present: false })
    await nextTick()
    expect(evSectionVisible.value).toBe(false)
  })
})
