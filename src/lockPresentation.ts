import type { DashboardControlView } from './dashboardControlView'

type Translate = (key: string) => string

export function isStableLock(control: DashboardControlView) {
  return control.lockState === 'locked' || control.lockState === 'unlocked'
}

export function createLockPresentation(t: Translate, label = (value: string) => value) {
  function lockStatus(control: DashboardControlView) {
    if (!control.lockState) return undefined
    const state = t(`lock.${control.lockState}`)
    if (control.pending) return `${state} · ${t('lock.pending')}`
    if (control.failed) return `${state} · ${t('lock.notConfirmed')}`
    return state
  }

  function lockAccessibility(control: DashboardControlView) {
    if (!control.lockState) return {}
    const description = `${label(control.label)}: ${lockStatus(control)}${control.failed ? `. ${t('lock.unconfirmed')}` : ''}`
    return { 'aria-label': description, title: description }
  }

  return { lockStatus, lockAccessibility }
}
