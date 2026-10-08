import type { Component } from 'vue'
import type { ControlState, DashboardControl } from './inverterControl'

export type LockState =
  'locked' | 'unlocked' | 'locking' | 'unlocking' | 'jammed' | 'unknown' | 'unavailable'

/** A rendered control may delegate to an optional provider without changing core transport. */
export interface DashboardControlView extends DashboardControl {
  state?: ControlState
  lockState?: LockState
  disabled?: boolean
  pending?: boolean
  failed?: boolean
  icon?: Component
  activate?: () => void
}
