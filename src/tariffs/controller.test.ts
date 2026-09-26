import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { saveControllerTariff } from './controller'
import { newDraft, rateGrid, validatePlan } from './model'

const native = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: native.invoke }))
beforeEach(() => {
  native.invoke.mockReset()
  vi.useFakeTimers()
})
afterEach(() => vi.useRealTimers())
const plan = () => validatePlan({ ...newDraft(), rates: rateGrid(0.2) })

it('waits for the matching controller acknowledgement instead of HTTP acceptance', async () => {
  let requestId = ''
  let reads = 0
  native.invoke.mockImplementation(async (name, args) => {
    if (name === 'perform_action') {
      requestId = args.payload.request_id
      return
    }
    return {
      ui_config: {
        electricity_tariff_status: {
          request_id: ++reads > 1 ? requestId : 'older-request',
          error: null,
        },
      },
    }
  })
  const saved = vi.fn()
  const operation = saveControllerTariff(plan(), 'a'.repeat(64)).then(saved)
  await vi.advanceTimersByTimeAsync(0)
  expect(saved).not.toHaveBeenCalled()
  await vi.advanceTimersByTimeAsync(200)
  await operation
  expect(saved).toHaveBeenCalledOnce()
  expect(native.invoke.mock.calls.filter(([name]) => name === 'perform_action')).toHaveLength(1)
})

it('surfaces rejection and never retries a possibly accepted write', async () => {
  let requestId = ''
  native.invoke.mockImplementation(async (name, args) => {
    if (name === 'perform_action') {
      requestId = args.payload.request_id
      return
    }
    return {
      ui_config: { electricity_tariff_status: { request_id: requestId, error: 'tariff changed' } },
    }
  })
  await expect(saveControllerTariff(null, 'a'.repeat(64))).rejects.toThrow('tariff changed')
  expect(native.invoke.mock.calls.filter(([name]) => name === 'perform_action')).toHaveLength(1)
})

it('times out without a matching acknowledgement', async () => {
  native.invoke.mockResolvedValue({ ui_config: {} })
  const operation = expect(saveControllerTariff(plan(), 'a'.repeat(64))).rejects.toThrow(
    'not confirmed'
  )
  await vi.advanceTimersByTimeAsync(10_000)
  await operation
})
