import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { defineComponent, h, onMounted, onUnmounted } from 'vue'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import AuthGate from '../components/AuthGate.vue'

const boundary = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: boundary.invoke }))
vi.mock('@tauri-apps/api/event', () => ({ listen: boundary.listen }))

type Callback = () => void
let callbacks: Set<Callback>
let mounted: ReturnType<typeof vi.fn>
let unmounted: ReturnType<typeof vi.fn>
let stop: ReturnType<typeof vi.fn>
let wrapper: VueWrapper | undefined
const AuthScreenStub = defineComponent({
  emits: ['authenticated'],
  template: '<div data-testid="auth-screen">Sign in</div>',
})
function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => {
    resolve = done
  })
  return { promise, resolve }
}
function emitAuthChanged() {
  for (const callback of callbacks) callback()
}
function renderGate() {
  const protectedPage = defineComponent({
    setup() {
      onMounted(mounted)
      onUnmounted(unmounted)
      return () => h('div', { 'data-testid': 'protected-content' }, 'Protected dashboard/settings')
    },
  })
  wrapper = mount(AuthGate, {
    slots: { default: () => h(protectedPage) },
    global: { stubs: { AuthScreen: AuthScreenStub } },
  })
  return wrapper
}
beforeEach(() => {
  vi.useFakeTimers()
  callbacks = new Set()
  mounted = vi.fn()
  unmounted = vi.fn()
  stop = vi.fn()
  boundary.invoke.mockReset().mockResolvedValue({ unlocked: false })
  boundary.listen.mockReset().mockImplementation(async (_event: string, callback: Callback) => {
    callbacks.add(callback)
    stop.mockImplementation(() => {
      callbacks.delete(callback)
    })
    return stop
  })
})
afterEach(() => {
  wrapper?.unmount()
  wrapper = undefined
  vi.useRealTimers()
})

describe('AuthGate protected component lifecycle', () => {
  it('does not mount App/Config before the authoritative status response', async () => {
    const pending = deferred<{ unlocked: boolean }>()
    boundary.invoke.mockReturnValue(pending.promise)
    const gate = renderGate()
    await flushPromises()
    expect(boundary.invoke).toHaveBeenCalledWith('auth_status')
    expect(mounted).not.toHaveBeenCalled()
    expect(gate.find('[data-testid="protected-content"]').exists()).toBe(false)
    pending.resolve({ unlocked: false })
    await flushPromises()
    expect(mounted).not.toHaveBeenCalled()
    expect(gate.find('[data-testid="auth-screen"]').exists()).toBe(true)
  })

  it('mounts protected content after a backend unlock event', async () => {
    const gate = renderGate()
    await flushPromises()
    expect(mounted).not.toHaveBeenCalled()
    boundary.invoke.mockResolvedValue({ unlocked: true })
    emitAuthChanged()
    await flushPromises()
    expect(mounted).toHaveBeenCalledTimes(1)
    expect(gate.find('[data-testid="protected-content"]').exists()).toBe(true)
  })

  it('immediately unmounts on revoke and stays locked while status is pending', async () => {
    boundary.invoke.mockResolvedValue({ unlocked: true })
    const gate = renderGate()
    await flushPromises()
    const pending = deferred<{ unlocked: boolean }>()
    boundary.invoke.mockReturnValue(pending.promise)
    emitAuthChanged()
    await flushPromises()
    expect(unmounted).toHaveBeenCalledTimes(1)
    expect(gate.find('[data-testid="protected-content"]').exists()).toBe(false)
    pending.resolve({ unlocked: false })
    await flushPromises()
    expect(gate.find('[data-testid="auth-screen"]').exists()).toBe(true)
  })

  it('fails closed and offers retry when periodic status IPC fails', async () => {
    boundary.invoke.mockResolvedValue({ unlocked: true })
    const gate = renderGate()
    await flushPromises()
    boundary.invoke.mockRejectedValue(new Error('Session check failed'))
    await vi.advanceTimersByTimeAsync(15_000)
    expect(unmounted).toHaveBeenCalledTimes(1)
    expect(gate.find('[data-testid="protected-content"]').exists()).toBe(false)
    expect(gate.text()).toContain('Unable to open settings: Error: Session check failed')
    expect(gate.get('button').text()).toBe('Retry')
  })

  it('rejects an old unlocked response arriving after a newer revoked response', async () => {
    const oldStatus = deferred<{ unlocked: boolean }>()
    boundary.invoke.mockReturnValueOnce(oldStatus.promise).mockResolvedValue({ unlocked: false })
    const gate = renderGate()
    await flushPromises()
    emitAuthChanged()
    await flushPromises()
    oldStatus.resolve({ unlocked: true })
    await flushPromises()
    expect(mounted).not.toHaveBeenCalled()
    expect(gate.find('[data-testid="auth-screen"]').exists()).toBe(true)
  })

  it('handles listener registration failure without invoking status and retries the subscription', async () => {
    boundary.listen.mockRejectedValueOnce(new Error('Event API unavailable'))
    const gate = renderGate()
    await flushPromises()
    expect(mounted).not.toHaveBeenCalled()
    expect(boundary.invoke).not.toHaveBeenCalled()
    expect(gate.text()).toContain('Event API unavailable')
    boundary.invoke.mockResolvedValue({ unlocked: true })
    await gate.get('button').trigger('click')
    await flushPromises()
    expect(boundary.listen).toHaveBeenCalledTimes(2)
    expect(mounted).toHaveBeenCalledTimes(1)
  })

  it('cleans up the event subscription and polling timer on unmount', async () => {
    const gate = renderGate()
    await flushPromises()
    gate.unmount()
    wrapper = undefined
    expect(stop).toHaveBeenCalledTimes(1)
    expect(callbacks.size).toBe(0)
    boundary.invoke.mockClear()
    await vi.advanceTimersByTimeAsync(60_000)
    expect(boundary.invoke).not.toHaveBeenCalled()
    expect(vi.getTimerCount()).toBe(0)
  })

  it('removes a late subscription and never starts IPC after early unmount', async () => {
    const pendingListener = deferred<() => void>()
    boundary.listen.mockReturnValue(pendingListener.promise)
    const gate = renderGate()
    gate.unmount()
    wrapper = undefined
    const lateStop = vi.fn()
    pendingListener.resolve(lateStop)
    await flushPromises()
    expect(lateStop).toHaveBeenCalledTimes(1)
    expect(boundary.invoke).not.toHaveBeenCalled()
    expect(vi.getTimerCount()).toBe(0)
  })
})
