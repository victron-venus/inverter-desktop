import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import SetpointOverride from '../components/SetpointOverride.vue'
import {
  activateTransportSession,
  deactivateTransportSession,
  getCurrentTransportSession,
  invalidateTransportSession,
  reserveTransportSession,
} from '../composables/transportSession'

const boundary = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn(), session: 'current' }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: boundary.invoke }))
vi.mock('@tauri-apps/api/event', () => ({ listen: boundary.listen }))

type Status = { value: number | null; last_error: string | null }
type Envelope = { notification_session: string; status: Status | null }
const scoped = (status: Status | null, session = boundary.session): Envelope => ({
  notification_session: session,
  status,
})
let onStatus: (event: { payload: Envelope }) => void
let onConnection: (event: {
  payload: { connected: boolean; notification_session?: string }
}) => void
let wrapper: VueWrapper | undefined
const inactive = { value: null, last_error: null }
const active = { value: 100, last_error: null }

beforeEach(() => {
  boundary.session = reserveTransportSession()
  activateTransportSession(boundary.session)
  boundary.invoke.mockReset().mockResolvedValue(scoped(inactive))
  boundary.listen.mockReset().mockImplementation(async (name, callback) => {
    if (name === 'setpoint-override-update') onStatus = callback
    if (name === 'mqtt-connection-status') onConnection = callback
    return vi.fn()
  })
})
afterEach(() => {
  wrapper?.unmount()
  wrapper = undefined
  invalidateTransportSession(getCurrentTransportSession())
  document.body.innerHTML = ''
})
async function render() {
  wrapper = mount(SetpointOverride, { props: { currentSetpoint: 25 }, attachTo: document.body })
  await flushPromises()
  return wrapper
}
async function open() {
  await wrapper!.get('button[aria-label="Setpoint override"]').trigger('click')
  await flushPromises()
}
const trigger = () => wrapper!.get('button[aria-label="Setpoint override"]')
const dialog = () => document.querySelector('dialog')!

describe('Setpoint Override transport status', () => {
  it('keeps one live status node across available and unavailable updates', async () => {
    const rendering = render()
    const node = wrapper!.get('[role="status"]').element
    await rendering
    expect(wrapper!.get('[role="status"]').element).toBe(node)
    expect(node.textContent?.trim()).toBe('')
    expect(node.getAttribute('aria-atomic')).toBe('true')
    onStatus({ payload: scoped(null) })
    await flushPromises()
    expect(wrapper!.get('[role="status"]').element).toBe(node)
    expect(node.textContent?.trim()).toBe('Status unknown')
    onStatus({ payload: scoped(active) })
    await flushPromises()
    expect(wrapper!.get('[role="status"]').element).toBe(node)
    expect(node.textContent?.trim()).toBe('')
    onConnection({ payload: { connected: false, notification_session: boundary.session } })
    await flushPromises()
    expect(wrapper!.get('[role="status"]').element).toBe(node)
    expect(node.textContent?.trim()).toBe('Status unknown')
  })
  it('keeps failed initial status unknown and disallows submission', async () => {
    boundary.invoke.mockRejectedValue(new Error('Gateway status unavailable'))
    await render()
    expect(trigger().attributes('aria-pressed')).toBeUndefined()
    expect(wrapper!.text()).toContain('Status unknown')
    await open()
    expect(dialog().querySelector<HTMLButtonElement>('button[type="submit"]')!.disabled).toBe(true)
    expect(boundary.invoke).toHaveBeenCalledTimes(1)
  })

  it('distinguishes lost status from an acknowledged inactive override and recovers on events', async () => {
    boundary.invoke.mockResolvedValue(scoped(active))
    await render()
    expect(trigger().attributes('aria-pressed')).toBe('true')
    expect(wrapper!.text()).toContain('100 W')
    onStatus({ payload: scoped(null) })
    await flushPromises()
    expect(trigger().attributes('aria-pressed')).toBeUndefined()
    expect(wrapper!.text()).toContain('Status unknown')
    expect(wrapper!.text()).not.toContain('100 W')
    await open()
    expect(dialog().textContent).not.toContain('Stop override')
    expect(dialog().querySelector<HTMLButtonElement>('button[type="submit"]')!.disabled).toBe(true)
    onStatus({ payload: scoped(inactive) })
    await flushPromises()
    expect(trigger().attributes('aria-pressed')).toBe('false')
    expect(wrapper!.text()).not.toContain('Status unknown')
    expect(dialog().querySelector<HTMLButtonElement>('button[type="submit"]')!.disabled).toBe(false)
  })

  it('does not let a delayed cached getter erase a newer unavailable event', async () => {
    let resolve!: (status: Envelope) => void
    boundary.invoke.mockReturnValue(
      new Promise<Envelope>((done) => {
        resolve = done
      })
    )
    await render()
    onStatus({ payload: scoped(null) })
    resolve(scoped(active))
    await flushPromises()
    expect(trigger().attributes('aria-pressed')).toBeUndefined()
    expect(wrapper!.text()).toContain('Status unknown')
  })

  it('does not let a delayed getter failure erase a newer live event', async () => {
    let reject!: (error: Error) => void
    boundary.invoke.mockReturnValue(
      new Promise<Envelope>((_done, fail) => {
        reject = fail
      })
    )
    await render()
    onStatus({ payload: scoped(active) })
    reject(new Error('Earlier request failed'))
    await flushPromises()
    expect(trigger().attributes('aria-pressed')).toBe('true')
    expect(wrapper!.text()).toContain('100 W')
    expect(wrapper!.text()).not.toContain('Earlier request failed')
  })

  it('keeps the dialog open after missing acknowledgement and sends only once', async () => {
    boundary.invoke.mockImplementation(async (command) => {
      if (command === 'set_setpoint_override')
        throw new Error('Cerbo has not confirmed the override')
      return scoped(inactive)
    })
    await render()
    await open()
    const input = dialog().querySelector<HTMLInputElement>('input')!
    input.value = '-250'
    input.dispatchEvent(new Event('input', { bubbles: true }))
    dialog()
      .querySelector('form')!
      .dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))
    await flushPromises()
    expect(dialog().textContent).toContain('Cerbo has not confirmed')
    expect(
      boundary.invoke.mock.calls.filter(([command]) => command === 'set_setpoint_override')
    ).toEqual([['set_setpoint_override', { value: -250, notificationSession: boundary.session }]])
  })

  it('reconciles acknowledged stop with fresh status instead of optimistic state', async () => {
    let current: Status = active
    boundary.invoke.mockImplementation(async (command, args) => {
      if (command === 'set_setpoint_override') current = { value: args.value, last_error: null }
      return scoped(current)
    })
    await render()
    await open()
    const stop = [...dialog().querySelectorAll('button')].find((button) =>
      button.textContent?.includes('Stop override')
    )!
    stop.click()
    await flushPromises()
    expect(boundary.invoke).toHaveBeenCalledWith('set_setpoint_override', {
      value: null,
      notificationSession: boundary.session,
    })
    expect(
      boundary.invoke.mock.calls.filter(([command]) => command === 'get_setpoint_override')
    ).toHaveLength(2)
    expect(trigger().attributes('aria-pressed')).toBe('false')
    expect(document.querySelector('dialog')).toBeNull()
  })
  it('clears an active status on transport loss until a fresh status arrives', async () => {
    boundary.invoke.mockResolvedValue(scoped(active))
    await render()
    onConnection({ payload: { connected: false, notification_session: boundary.session } })
    await flushPromises()
    expect(trigger().attributes('aria-pressed')).toBeUndefined()
    expect(wrapper!.text()).toContain('Status unknown')
    onConnection({ payload: { connected: true, notification_session: boundary.session } })
    await flushPromises()
    expect(trigger().attributes('aria-pressed')).toBeUndefined()
    onStatus({ payload: scoped(inactive) })
    await flushPromises()
    expect(trigger().attributes('aria-pressed')).toBe('false')
  })

  it('does not restore old command status after a newer source-loss event', async () => {
    let resolveCommand!: (status: Envelope) => void
    let resolveRefresh!: (status: Envelope) => void
    let gets = 0
    boundary.invoke.mockImplementation((command) => {
      if (command === 'set_setpoint_override') {
        return new Promise<Envelope>((done) => {
          resolveCommand = done
        })
      }
      if (++gets === 1) return Promise.resolve(scoped(active))
      return new Promise<Envelope>((done) => {
        resolveRefresh = done
      })
    })
    await render()
    await open()
    dialog()
      .querySelector('form')!
      .dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))
    await flushPromises()
    onConnection({ payload: { connected: false, notification_session: boundary.session } })
    resolveCommand(scoped(active))
    await flushPromises()
    expect(trigger().attributes('aria-pressed')).toBeUndefined()
    expect(wrapper!.text()).toContain('Status unknown')
    // A genuinely new status event still beats the subsequent getter.
    onStatus({ payload: scoped(inactive) })
    resolveRefresh(scoped(active))
    await flushPromises()
    expect(trigger().attributes('aria-pressed')).toBe('false')
  })

  it('ignores old-source override values and withdrawals after a live replacement', async () => {
    boundary.invoke.mockResolvedValue(scoped(active))
    await render()
    const previous = boundary.session
    boundary.session = reserveTransportSession()
    activateTransportSession(boundary.session)
    await flushPromises()
    expect(wrapper!.text()).toContain('Status unknown')
    onStatus({ payload: scoped({ value: 250, last_error: null }) })
    onStatus({ payload: scoped(active, previous) })
    onStatus({ payload: scoped(null, previous) })
    onStatus({ payload: active as unknown as Envelope })
    onStatus({ payload: null as unknown as Envelope })
    await flushPromises()
    expect(wrapper!.text()).toContain('250 W')
    expect(wrapper!.text()).not.toContain('100 W')
    expect(wrapper!.text()).not.toContain('Status unknown')
  })

  it.each(['reserve', 'deactivate'] as const)(
    'withdraws readiness immediately on %s before native reset',
    async (transition) => {
      boundary.invoke.mockResolvedValue(scoped(active))
      await render()
      await open()
      if (transition === 'reserve') reserveTransportSession()
      else deactivateTransportSession(boundary.session)
      // Submit without allowing a render tick: the identity watch is synchronous.
      dialog()
        .querySelector('form')!
        .dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))
      await flushPromises()
      expect(wrapper!.text()).toContain('Status unknown')
      expect(dialog().querySelector<HTMLButtonElement>('button[type="submit"]')!.disabled).toBe(
        true
      )
      expect(
        boundary.invoke.mock.calls.filter(([command]) => command === 'set_setpoint_override')
      ).toHaveLength(0)
    }
  )

  it.each(['success', 'failure'] as const)(
    'ignores a previous transport getter %s without needing a newer event',
    async (result) => {
      const previous = boundary.session
      let resolve!: (status: Envelope) => void
      let reject!: (error: Error) => void
      boundary.invoke.mockReturnValue(
        new Promise<Envelope>((done, fail) => {
          resolve = done
          reject = fail
        })
      )
      await render()
      boundary.session = reserveTransportSession()
      activateTransportSession(boundary.session)
      if (result === 'success') resolve(scoped(active, previous))
      else reject(new Error('Old endpoint failed'))
      await flushPromises()
      expect(wrapper!.text()).toContain('Status unknown')
      expect(wrapper!.text()).not.toContain('100 W')
      expect(wrapper!.text()).not.toContain('Old endpoint failed')
    }
  )

  it('keeps Retry unavailable after disconnect until a fresh override is observed', async () => {
    boundary.invoke.mockResolvedValue(scoped(active))
    await render()
    onConnection({ payload: { connected: false, notification_session: boundary.session } })
    boundary.invoke.mockRejectedValue(new Error('Waiting for fresh MQTT telemetry'))
    await open()
    const retry = [...dialog().querySelectorAll('button')].find((button) =>
      button.textContent?.includes('Retry')
    )!
    retry.click()
    await flushPromises()
    expect(wrapper!.text()).toContain('Status unknown')
    expect(dialog().querySelector<HTMLButtonElement>('button[type="submit"]')!.disabled).toBe(true)
    onConnection({ payload: { connected: true, notification_session: boundary.session } })
    await flushPromises()
    expect(dialog().querySelector<HTMLButtonElement>('button[type="submit"]')!.disabled).toBe(true)
    onStatus({ payload: scoped(inactive) })
    await flushPromises()
    expect(dialog().querySelector<HTMLButtonElement>('button[type="submit"]')!.disabled).toBe(false)
  })

  it.each(['success', 'failure'] as const)(
    'does not replay or reconcile an old command %s on a replacement',
    async (result) => {
      const previous = boundary.session
      let resolve!: (status: Envelope) => void
      let reject!: (error: Error) => void
      boundary.invoke.mockImplementation((command) =>
        command === 'set_setpoint_override'
          ? new Promise<Envelope>((done, fail) => {
              resolve = done
              reject = fail
            })
          : Promise.resolve(scoped(active))
      )
      await render()
      await open()
      dialog()
        .querySelector('form')!
        .dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))
      await flushPromises()
      boundary.session = reserveTransportSession()
      activateTransportSession(boundary.session)
      onStatus({ payload: scoped({ value: 250, last_error: null }) })
      if (result === 'success') resolve(scoped(active, previous))
      else reject(new Error('Old command failed'))
      await flushPromises()
      expect(wrapper!.text()).toContain('250 W')
      expect(dialog()).not.toBeNull()
      expect(dialog().textContent).not.toContain('Old command failed')
      expect(
        boundary.invoke.mock.calls.filter(([command]) => command === 'get_setpoint_override')
      ).toHaveLength(1)
      expect(
        boundary.invoke.mock.calls.filter(([command]) => command === 'set_setpoint_override')
      ).toEqual([['set_setpoint_override', { value: 100, notificationSession: previous }]])
    }
  )

  it('ignores delayed connection loss from a replaced transport and untagged status', async () => {
    boundary.invoke.mockResolvedValue(scoped(active))
    await render()
    onConnection({ payload: { connected: false, notification_session: 'previous' } })
    onConnection({ payload: { connected: false } })
    await flushPromises()
    expect(trigger().attributes('aria-pressed')).toBe('true')
    expect(wrapper!.text()).toContain('100 W')
    onConnection({ payload: { connected: false, notification_session: boundary.session } })
    await flushPromises()
    expect(wrapper!.text()).toContain('Status unknown')
  })
})
