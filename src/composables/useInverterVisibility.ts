import { invoke } from '@tauri-apps/api/core'
import { logger } from '../logger'
import { applyInverterState, type InverterState } from './useInverterState'

/** Resume core telemetry independently of optional home-device integrations. */
export function useInverterVisibility() {
  let revision = 0

  async function setInverterWindowHidden(hidden: boolean) {
    const current = ++revision
    try {
      await invoke('set_window_hidden', { hidden })
      if (hidden || current !== revision) return
      const initial = await invoke<InverterState>('get_state')
      if (initial && current === revision) applyInverterState(initial, { snapshot: true })
    } catch (error) {
      logger.error('Failed to sync inverter window state:', error)
    }
  }

  function cleanupInverterVisibility() {
    revision += 1
  }

  return { setInverterWindowHidden, cleanupInverterVisibility }
}
