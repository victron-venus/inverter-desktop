import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import Config from '../Config.vue'
import { defaultConfig } from '../config'
import { logger } from '../logger'

// Keep the pinned Tauri event/core implementations: only their native IPC boundary is synthetic.
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ close: vi.fn() }) }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))
const FEATURE_EVENT = 'plugin-configuration-changed'
const LISTEN_COMMAND = 'plugin:event|listen'
const UNLISTEN_COMMAND = 'plugin:event|unlisten'
const OPTIONAL_WARNING = 'Optional settings updates are unavailable'
let wrapper: VueWrapper | undefined
let resolveRegistration: (id: number) => void
let rejectRegistration: (error: Error) => void
let registration: Promise<number>
const invoke = vi.fn()
const unregisterListener = vi.fn()
const transformCallback = vi.fn()

beforeEach(() => {
  registration = new Promise<number>((resolve, reject) => {
    resolveRegistration = resolve
    rejectRegistration = reject
  })
  invoke.mockReset().mockImplementation((command: string, args: { event?: string }) => {
    if (command === 'get_config') return Promise.resolve(structuredClone(defaultConfig))
    if (command === 'get_state') return Promise.resolve({})
    if (command === LISTEN_COMMAND)
      return args.event === FEATURE_EVENT ? registration : Promise.resolve(42)
    return Promise.resolve(undefined)
  })
  unregisterListener.mockReset()
  transformCallback.mockReset().mockReturnValue(7)
  vi.stubGlobal('__TAURI_INTERNALS__', { invoke, transformCallback })
  vi.stubGlobal('__TAURI_EVENT_PLUGIN_INTERNALS__', { unregisterListener })
  vi.spyOn(logger, 'warn').mockImplementation(() => {})
})

afterEach(async () => {
  wrapper?.unmount()
  wrapper = undefined
  await flushPromises()
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
})

function unmount() {
  wrapper?.unmount()
  wrapper = undefined
}

function expectNoDiscovery() {
  expect(invoke.mock.calls.some(([command]) => command === 'get_state')).toBe(false)
}

describe('real Tauri subscription at the Config lifecycle boundary', () => {
  it.each(['before-resolution', 'after-resolution'])(
    'releases a late subscription when unmounted %s without continuing discovery',
    async (timing) => {
      wrapper = mount(Config)
      await flushPromises()
      expect(invoke).toHaveBeenCalledWith(
        LISTEN_COMMAND,
        expect.objectContaining({ event: FEATURE_EVENT }),
        undefined
      )
      expectNoDiscovery()
      if (timing === 'before-resolution') unmount()
      resolveRegistration(41)
      if (timing === 'after-resolution') unmount()
      await flushPromises()
      expect(unregisterListener.mock.calls).toEqual([[FEATURE_EVENT, 41]])
      expect(invoke).toHaveBeenCalledWith(
        UNLISTEN_COMMAND,
        { event: FEATURE_EVENT, eventId: 41 },
        undefined
      )
      expectNoDiscovery()
      expect(logger.warn).not.toHaveBeenCalled()
    }
  )

  it('ignores registration rejection after unmount and never starts discovery', async () => {
    wrapper = mount(Config)
    await flushPromises()
    unmount()
    rejectRegistration(new Error('synthetic registration failure'))
    await flushPromises()
    expectNoDiscovery()
    expect(unregisterListener).not.toHaveBeenCalled()
    expect(logger.warn).not.toHaveBeenCalled()
  })

  it.each(['callback', 'invoke'])(
    'handles a synchronous native %s failure as a rejected optional subscription',
    async (stage) => {
      if (stage === 'callback') {
        transformCallback.mockImplementationOnce(() => {
          throw new Error('synthetic callback failure')
        })
      } else {
        const normal = invoke.getMockImplementation()
        if (!normal) throw new Error('Missing synthetic native boundary')
        invoke.mockImplementation((command, args, options) => {
          if (command === LISTEN_COMMAND && args.event === FEATURE_EVENT)
            throw new Error('synthetic invoke failure')
          return normal(command, args, options)
        })
      }
      wrapper = mount(Config)
      await flushPromises()
      expect(logger.warn).toHaveBeenCalledWith(OPTIONAL_WARNING)
      expect(invoke.mock.calls.some(([command]) => command === 'get_state')).toBe(true)
      unmount()
      await flushPromises()
      expect(unregisterListener.mock.calls).toEqual([['mqtt-state-update', 42]])
    }
  )

  it('releases both completed subscriptions exactly once on unmount', async () => {
    resolveRegistration(41)
    wrapper = mount(Config)
    await flushPromises()
    unmount()
    await flushPromises()
    expect(unregisterListener.mock.calls).toEqual([
      [FEATURE_EVENT, 41],
      ['mqtt-state-update', 42],
    ])
    expect(logger.warn).not.toHaveBeenCalled()
  })
})
