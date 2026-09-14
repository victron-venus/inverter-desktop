import { flushPromises } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { resetInverterState, state } from '../composables/useInverterState'
import { useInverterVisibility } from '../composables/useInverterVisibility'

const boundary = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: boundary.invoke }))

beforeEach(() => {
  resetInverterState()
  boundary.invoke.mockReset()
})

describe('core telemetry window lifecycle', () => {
  it('restores inverter telemetry without any HA calls', async () => {
    boundary.invoke.mockImplementation(async (command: string) =>
      command === 'get_state' ? { battery_soc: 76 } : undefined
    )
    const core = useInverterVisibility()
    await core.setInverterWindowHidden(false)
    expect(state.value.battery_soc).toBe(76)
    expect(boundary.invoke.mock.calls.map(([command]) => command)).toEqual([
      'set_window_hidden',
      'get_state',
    ])
  })

  it.each(['hidden', 'unmounted'])(
    'discards a late snapshot after the window is %s',
    async (reason) => {
      let resolve!: (value: unknown) => void
      const snapshot = new Promise((done) => {
        resolve = done
      })
      boundary.invoke.mockImplementation(async (command: string) =>
        command === 'get_state' ? snapshot : undefined
      )
      const core = useInverterVisibility()
      const pending = core.setInverterWindowHidden(false)
      await flushPromises()
      if (reason === 'hidden') await core.setInverterWindowHidden(true)
      else core.cleanupInverterVisibility()
      resolve({ battery_soc: 99 })
      await pending
      expect(state.value.battery_soc).not.toBe(99)
    }
  )
})
