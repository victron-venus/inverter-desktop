import type { Component } from 'vue'
import type { ControlState, DashboardControl } from './inverterControl'

/** A rendered control may delegate to an optional provider without changing core transport. */
export interface DashboardControlView extends DashboardControl {
  state?: ControlState
  disabled?: boolean
  pending?: boolean
  failed?: boolean
  icon?: Component
  activate?: () => void
}
