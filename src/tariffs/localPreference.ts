import { isTauri } from '@tauri-apps/api/core'
import { emit } from '@tauri-apps/api/event'
import { clearTariff, loadTariff, saveTariff } from './storage'
import type { TariffPlan } from './model'

export const LOCAL_TARIFF_EVENT = 'local-tariff-changed'
export type TariffMode = 'controller' | 'local'
export type LocalTariffChange =
  | { scope: string; kind: 'mode'; value: TariffMode }
  | { scope: string; kind: 'plan'; value: TariffPlan | null }

export function tariffModeKey(scope: string): string {
  return `victron.energy-tariff-mode.v1:${scope}`
}

export function loadLocalPreference(scope: string) {
  try {
    const savedMode = localStorage.getItem(tariffModeKey(scope))
    if (savedMode !== null && savedMode !== 'controller' && savedMode !== 'local')
      throw new Error('Invalid tariff mode')
    const mode: TariffMode = savedMode === 'local' ? 'local' : 'controller'
    return { mode, ...loadTariff(scope) }
  } catch {
    return {
      mode: 'controller' as TariffMode,
      plan: null,
      error: 'The tariff preference could not be loaded on this device.',
    }
  }
}

/** Native notifications invalidate a scope; persisted storage remains authoritative. */
export function localTariffEventScope(value: unknown): string | null {
  if (!value || typeof value !== 'object') return null
  const { scope } = value as Record<string, unknown>
  return typeof scope === 'string' && scope ? scope : null
}

function applyLocalTariffChange(change: LocalTariffChange) {
  if (change.kind === 'mode') {
    try {
      if (localStorage.getItem(tariffModeKey(change.scope)) !== change.value)
        localStorage.setItem(tariffModeKey(change.scope), change.value)
    } catch {
      throw new Error('The tariff preference could not be saved on this device.')
    }
  } else if (change.value === null) clearTariff(change.scope)
  else saveTariff(change.scope, change.value)
  window.dispatchEvent(new CustomEvent(LOCAL_TARIFF_EVENT, { detail: change.scope }))
}

export async function saveLocalPreference(change: LocalTariffChange) {
  applyLocalTariffChange(change)
  if (!isTauri()) return
  try {
    await emit(LOCAL_TARIFF_EVENT, { scope: change.scope })
  } catch {
    throw new Error(
      'Saved on this device, but other open windows could not be updated. Reopen those windows to load the saved tariff.'
    )
  }
}
