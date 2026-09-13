import { describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import { applyInverterState, mqttConnected, state } from '../composables/useInverterState'
import { useMQTTState } from '../composables/useMQTTState'
import { resolveHeaderToggleState } from '../utils'

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

function computeToggleStates(
  headerToggles: Array<{ id: string; entity: string }>,
  haEntityStates: Record<string, string>,
  haEnabled: boolean,
  mqttBooleans: Record<string, unknown>
): Record<string, string> {
  const states: Record<string, string> = {}
  for (const toggle of headerToggles) {
    states[toggle.id] = resolveHeaderToggleState(toggle, haEntityStates, haEnabled, mqttBooleans)
  }
  return states
}

describe('computeToggleStates', () => {
  const toggles = [
    { id: 'only_charging', entity: 'input_boolean.only_charging' },
    { id: 'no_feed', entity: 'input_boolean.no_feed' },
  ]

  it('ignores HA entity state for inverter-control flags even when HA enabled', () => {
    const states = computeToggleStates(
      toggles,
      { 'input_boolean.only_charging': 'on', 'input_boolean.no_feed': 'off' },
      true,
      { only_charging: false, no_feed: true }
    )
    expect(states.only_charging).toBe('off')
    expect(states.no_feed).toBe('on')
  })

  it('falls back to MQTT booleans when HA disabled', () => {
    const states = computeToggleStates(toggles, {}, false, {
      only_charging: true,
      no_feed: false,
    })
    expect(states.only_charging).toBe('on')
    expect(states.no_feed).toBe('off')
  })

  it('handles string values from MQTT', () => {
    const states = computeToggleStates(toggles, {}, false, {
      only_charging: 'true',
      no_feed: 'false',
    })
    expect(states.only_charging).toBe('on')
    expect(states.no_feed).toBe('off')
  })

  it('handles numeric values from MQTT', () => {
    const states = computeToggleStates(toggles, {}, false, {
      only_charging: 1,
      no_feed: 0,
    })
    expect(states.only_charging).toBe('on')
    expect(states.no_feed).toBe('off')
  })

  it('defaults to off when no data', () => {
    const states = computeToggleStates(toggles, {}, false, {})
    expect(states.only_charging).toBe('off')
    expect(states.no_feed).toBe('off')
  })
})

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
