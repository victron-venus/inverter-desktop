import type { DashboardContribution } from './types'

type Control = Extract<DashboardContribution, { kind: 'action' | 'number_input' }>

export interface ContributionCard {
  item: DashboardContribution
  controls: Control[]
}

/** Group explicit references for display without changing the authorization snapshot. */
export function contributionCards(items: DashboardContribution[]): ContributionCard[] {
  const states = new Map<string, ContributionCard>()
  for (const item of items) {
    if (item.kind === 'text' || item.kind === 'metric' || item.kind === 'status') {
      states.set(item.id, { item, controls: [] })
    }
  }
  const cards: ContributionCard[] = []
  for (const item of items) {
    if (item.kind === 'action' || item.kind === 'number_input') {
      const state = item.state_id === undefined ? undefined : states.get(item.state_id)
      if (state) state.controls.push(item)
      else cards.push({ item, controls: [] })
    } else {
      const state = states.get(item.id)
      if (state) cards.push(state)
    }
  }
  return cards
}
