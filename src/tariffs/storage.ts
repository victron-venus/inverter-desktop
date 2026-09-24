import { validatePlan, type TariffPlan } from './model'

export function tariffKey(scope: string): string {
  return `victron.energy-tariff.v1:${scope}`
}

export function loadTariff(scope: string): { plan: TariffPlan | null; error: string } {
  try {
    const raw = localStorage.getItem(tariffKey(scope))
    return { plan: raw === null ? null : validatePlan(JSON.parse(raw)), error: '' }
  } catch {
    return {
      plan: null,
      error:
        'The saved tariff could not be loaded. Import or save a valid tariff to restore the estimate.',
    }
  }
}

export function saveTariff(scope: string, plan: TariffPlan): void {
  const validated = validatePlan(plan)
  try {
    localStorage.setItem(tariffKey(scope), JSON.stringify(validated))
  } catch {
    throw new Error(
      'The tariff could not be saved on this device. Your previous tariff is unchanged.'
    )
  }
}

export function clearTariff(scope: string): void {
  try {
    localStorage.removeItem(tariffKey(scope))
  } catch {
    throw new Error('The saved tariff could not be removed on this device.')
  }
}
