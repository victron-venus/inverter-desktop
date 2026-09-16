import { computed, inject, type InjectionKey } from 'vue'
import {
  Home,
  PlugZap,
  Lightbulb,
  WashingMachine,
  Wind,
  Utensils,
  Thermometer,
  Gauge,
  Blinds,
  Play,
  CloudSun,
} from '@lucide/vue'
import { dashboardControlSource } from '../../../composables/useDashboardControls'
import type { DashboardControlView } from '../../../dashboardControlView'
import type { DashboardControl } from '../../../inverterControl'
import type {
  ActionContribution,
  DashboardContribution,
  NumberInputContribution,
  PluginPresentation,
  PluginSnapshot,
  PresentationIcon,
} from './types'
import { createPluginDashboard } from './usePluginDashboard'

const icons = {
  home: Home,
  plug: PlugZap,
  light: Lightbulb,
  washer: WashingMachine,
  dryer: Wind,
  dishwasher: Utensils,
  thermometer: Thermometer,
  gauge: Gauge,
  blinds: Blinds,
  play: Play,
  cloud: CloudSun,
}
export function presentationIcon(icon?: PresentationIcon | null) {
  return icon ? icons[icon] : undefined
}
export function contribution(
  plugin: PluginSnapshot,
  id?: string | null
): DashboardContribution | undefined {
  return id ? plugin.contributions.find((item) => item.id === id) : undefined
}
export function actionContribution(
  plugin: PluginSnapshot,
  id?: string | null
): ActionContribution | undefined {
  const item = contribution(plugin, id)
  return item?.kind === 'action' ? item : undefined
}
export function numberContribution(
  plugin: PluginSnapshot,
  id?: string | null
): NumberInputContribution | undefined {
  const item = contribution(plugin, id)
  return item?.kind === 'number_input' ? item : undefined
}
export function contributionText(item?: DashboardContribution): string {
  if (item?.kind === 'text') return item.text
  if (item?.kind === 'metric') return `${item.value}${item.unit ?? ''}`
  if (item?.kind === 'status') return item.value
  return ''
}

export function createPluginPresentation() {
  const dashboard = createPluginDashboard()
  const entries = computed(() =>
    dashboard.plugins.value.flatMap((plugin) =>
      (plugin.presentation ?? []).map((item) => ({
        plugin,
        item,
        key: JSON.stringify([plugin.plugin_id, plugin.instance_id, item.id]),
      }))
    )
  )
  const sidebar = computed(() =>
    entries.value
      .filter(
        ({ item }) =>
          item.kind === 'group' ||
          item.kind === 'weather' ||
          (item.kind === 'summary' && item.visible)
      )
      .sort(
        (left, right) =>
          ('order' in left.item ? left.item.order : 0) -
          ('order' in right.item ? right.item.order : 0)
      )
  )
  const connections = computed(() =>
    entries.value.filter(
      (
        entry
      ): entry is typeof entry & { item: Extract<PluginPresentation, { kind: 'connection' }> } =>
        entry.item.kind === 'connection'
    )
  )

  function runAction(plugin: PluginSnapshot, id: string) {
    const action = actionContribution(plugin, id)
    if (action) void dashboard.runAction(plugin.plugin_id, plugin.instance_id, action)
  }
  function pending(plugin: PluginSnapshot, id: string) {
    const action = actionContribution(plugin, id)
    return (
      !!action &&
      dashboard.pendingActions.value.has(
        dashboard.actionKey(plugin.plugin_id, plugin.instance_id, action)
      )
    )
  }
  function failed(plugin: PluginSnapshot, id: string) {
    const action = actionContribution(plugin, id)
    return (
      !!action &&
      dashboard.failedActions.value.has(
        dashboard.actionKey(plugin.plugin_id, plugin.instance_id, action)
      )
    )
  }
  function mergeControls(
    surface: 'header' | 'home',
    core: DashboardControl[]
  ): DashboardControlView[] {
    const source = dashboardControlSource(surface)
    const controls: Array<{ order: number; control: DashboardControlView }> = core.map(
      (control, index) => ({
        order:
          source.findIndex((entry) => entry.id === control.id) < 0
            ? index
            : source.findIndex((entry) => entry.id === control.id),
        // Runtime callbacks never come from persisted opaque configuration fields.
        control: {
          id: control.id,
          label: control.label,
          entity: control.entity,
          state_key: control.state_key,
        },
      })
    )
    for (const { plugin, item } of entries.value) {
      if (item.kind !== 'control' || item.surface !== surface) continue
      const action = actionContribution(plugin, item.action)
      const available = dashboard.canAct(plugin) && !!action
      controls.push({
        order: item.order,
        control: {
          id: `${plugin.plugin_id}:${item.id}`,
          label: item.title,
          entity: '',
          state: dashboard.canAct(plugin) ? item.state : 'unavailable',
          disabled: !available,
          pending: item.action ? pending(plugin, item.action) : false,
          failed: item.action ? failed(plugin, item.action) : false,
          icon: presentationIcon(item.icon),
          activate: () => {
            if (item.action) runAction(plugin, item.action)
          },
        },
      })
    }
    return controls.sort((left, right) => left.order - right.order).map(({ control }) => control)
  }
  return { dashboard, sidebar, connections, mergeControls, runAction, pending, failed }
}

export type PluginPresentationContext = ReturnType<typeof createPluginPresentation>
export const pluginPresentationKey: InjectionKey<PluginPresentationContext> =
  Symbol('plugin-presentation')
export function usePluginPresentation() {
  const context = inject(pluginPresentationKey)
  if (!context) throw new Error('Plugin presentation requires the desktop dashboard provider')
  return context
}
