import { invoke } from '@tauri-apps/api/core'
import { computed } from 'vue'
import {
  type ControlState,
  type DashboardControl,
  isInverterControlFlag,
  mqttControlState,
  normalizeControlTarget,
} from '../inverterControl'
import { state } from './useInverterState'

/** Presentation is owned by inverter-control; local legacy definitions are migration data only. */
export function dashboardControlSource(surface: 'header' | 'home'): DashboardControl[] {
  return (
    (surface === 'header'
      ? state.value.ui_config?.header_toggles
      : state.value.ui_config?.home_buttons) ?? []
  )
}

/** Optional home-device adapter; inverter controls never consult it. */
export type HomeControlStateProvider = (control: DashboardControl) => ControlState | undefined

export function useDashboardControls(
  homeState?: HomeControlStateProvider,
  allowHomeControls = true
) {
  const headerControls = computed(() =>
    dashboardControlSource('header')
      .map(normalizeControlTarget)
      .filter((control) => allowHomeControls || isInverterControlFlag(control.entity))
  )

  const homeButtons = computed(() =>
    dashboardControlSource('home')
      .filter((control) => !('enabled' in control) || control.enabled !== false)
      .map(normalizeControlTarget)
      .filter(
        (control) =>
          (allowHomeControls && state.value.features?.ha !== false) ||
          isInverterControlFlag(control.entity)
      )
  )

  function resolve(control: DashboardControl, fallbackKey = control.id): ControlState {
    if (!isInverterControlFlag(control.entity)) {
      const value = homeState?.(control)
      if (value !== undefined) return value
    }
    return mqttControlState(control, state.value.booleans ?? {}, fallbackKey)
  }

  const headerControlStates = computed(() =>
    Object.fromEntries(headerControls.value.map((control) => [control.id, resolve(control)]))
  )
  const homeButtonStates = computed(() =>
    Object.fromEntries(
      homeButtons.value.map((control) => [control.id, resolve(control, `home_${control.id}`)])
    )
  )

  return { headerControls, headerControlStates, homeButtons, homeButtonStates }
}

/** Dispatch is part of the app core. Rust routes inverter commands to MQTT. */
export async function sendControlAction(action: string, payload: Record<string, unknown> = {}) {
  await invoke('perform_action', { action, payload })
}
