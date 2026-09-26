import { invoke } from '@tauri-apps/api/core'
import { validatePlan, type TariffPlan } from './model'

export interface ControllerTariff {
  plan: unknown
  status?: { writable?: boolean; revision?: string; request_id?: string; error?: string | null }
}

export async function readControllerTariff(): Promise<ControllerTariff> {
  const state = await invoke<{
    ui_config?: {
      electricity_tariff?: unknown
      electricity_tariff_status?: ControllerTariff['status']
    }
  }>('get_state')
  return {
    plan: state?.ui_config?.electricity_tariff,
    status: state?.ui_config?.electricity_tariff_status,
  }
}

/** Queue acceptance is not success: wait for the controller's durable-write ACK. */
export async function saveControllerTariff(
  plan: TariffPlan | null,
  revision: string
): Promise<void> {
  const requestId = crypto.randomUUID()
  await invoke('perform_action', {
    action: 'electricity_tariff',
    payload: {
      request_id: requestId,
      revision,
      plan: plan === null ? null : validatePlan(plan),
    },
  })
  const deadline = Date.now() + 10_000
  while (Date.now() < deadline) {
    const { status } = await readControllerTariff()
    if (status?.request_id === requestId) {
      if (status.error) throw new Error(status.error)
      return
    }
    await new Promise((resolve) => setTimeout(resolve, 200))
  }
  throw new Error(
    'The controller has not confirmed the tariff. Reload its current tariff before trying again.'
  )
}
