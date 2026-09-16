import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import CameraVideo from './PluginMedia.vue'

const native = vi.hoisted(() => ({
  invoke: vi.fn(),
  convertFileSrc: vi.fn(),
  getCurrentWindow: vi.fn(),
  close: vi.fn(),
  warn: vi.fn(),
  error: vi.fn(),
}))

vi.mock('@tauri-apps/api/core', () => ({
  invoke: native.invoke,
  convertFileSrc: native.convertFileSrc,
}))
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: native.getCurrentWindow }))
vi.mock('../../../logger', () => ({ logger: { warn: native.warn, error: native.error } }))

const id = 'acdb88b6-453a-4f88-8d52-5b902441fe4c'
const owner = `plugin-video-${id}`
const initialLocation = globalThis.location.href
const mounted: VueWrapper[] = []

async function player(params: Record<string, string>, label = owner) {
  globalThis.history.replaceState({}, '', `/camera-video?${new URLSearchParams(params)}`)
  native.getCurrentWindow.mockReturnValue({ label, close: native.close })
  const wrapper = mount(CameraVideo)
  mounted.push(wrapper)
  await flushPromises()
  return wrapper
}

beforeEach(() => {
  native.invoke.mockReset().mockResolvedValue(undefined)
  native.convertFileSrc.mockReset().mockImplementation((path: string, scheme?: string) => {
    return `${scheme || 'asset'}://localhost/${path}`
  })
  native.getCurrentWindow.mockReset()
  native.close.mockReset().mockResolvedValue(undefined)
  native.warn.mockReset()
  native.error.mockReset()
})

afterEach(() => {
  for (const wrapper of mounted.splice(0)) wrapper.unmount()
  vi.useRealTimers()
  globalThis.history.replaceState({}, '', initialLocation)
})

describe('native-owned plugin video player', () => {
  it('loads a live stream only through its zero-argument owner command and leaves lifetime native', async () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
    native.invoke.mockResolvedValue('https://camera.invalid/api/front?token=private')
    const wrapper = await player(
      { pluginMedia: id, pluginMediaKind: 'live', videoUrl: 'https://ignored.invalid' },
      `plugin-preview-${id}`
    )
    expect(native.invoke).toHaveBeenCalledExactlyOnceWith('get_live_preview_url')
    expect(native.convertFileSrc).not.toHaveBeenCalled()
    expect(wrapper.get('img').attributes('src')).toBe(
      'https://camera.invalid/api/front?token=private'
    )
    expect(wrapper.get('img').attributes('alt')).toBe('Live camera')
    expect(wrapper.find('iframe').exists()).toBe(false)
    expect(wrapper.find('video').exists()).toBe(false)
    await wrapper.get('img').trigger('load')
    await vi.advanceTimersByTimeAsync(12000)
    await wrapper.get('img').trigger('load')
    await vi.advanceTimersByTimeAsync(15000)
    expect(native.invoke).toHaveBeenCalledTimes(1)
    await wrapper.get('button[aria-label="Close"]').trigger('click')
    expect(native.invoke).toHaveBeenLastCalledWith('close_plugin_video_window')
    expect(native.warn).not.toHaveBeenCalled()
  })

  it('does not attach a late live response after the viewer unmounts', async () => {
    let resolve: (value: string) => void = () => {
      throw new Error('Live request was not initialized')
    }
    native.invoke.mockImplementation(
      () =>
        new Promise<string>((complete) => {
          resolve = complete
        })
    )
    const wrapper = await player(
      { pluginMedia: id, pluginMediaKind: 'live' },
      `plugin-preview-${id}`
    )
    const element = wrapper.element
    wrapper.unmount()
    mounted.splice(mounted.indexOf(wrapper), 1)
    resolve('https://private.invalid/frame?token=secret')
    await flushPromises()
    expect(element.querySelector('img')).toBeNull()
    expect(native.convertFileSrc).not.toHaveBeenCalled()
    expect(native.warn).not.toHaveBeenCalled()
  })

  it.each(['javascript:alert(1)', 'https://user:password@camera.invalid', '', undefined])(
    'rejects an invalid live descriptor without logging its value',
    async (value) => {
      native.invoke.mockResolvedValue(value)
      const wrapper = await player(
        { pluginMedia: id, pluginMediaKind: 'live' },
        `plugin-preview-${id}`
      )
      expect(wrapper.find('img').exists()).toBe(false)
      expect(wrapper.text()).toContain('Failed to open live camera preview.')
      expect(native.warn).toHaveBeenCalledExactlyOnceWith(
        'Camera clip error:',
        'Failed to open live camera preview.'
      )
    }
  )

  it('clears a failed live stream without exposing its URL or retaining a still timer', async () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
    native.invoke.mockResolvedValue('https://camera.invalid/frame?token=private')
    const wrapper = await player(
      { pluginMedia: id, pluginMediaKind: 'live' },
      `plugin-preview-${id}`
    )
    await wrapper.get('img').trigger('load')
    await wrapper.get('img').trigger('error')
    expect(wrapper.find('img').exists()).toBe(false)
    expect(wrapper.html()).not.toContain('private')
    expect(native.warn).toHaveBeenCalledExactlyOnceWith('Live camera preview failed')
    await vi.advanceTimersByTimeAsync(15000)
    expect(native.invoke).toHaveBeenCalledTimes(1)
  })

  it('resolves only the opaque handle and plays inline and muted despite legacy URL fields', async () => {
    const wrapper = await player({
      pluginMedia: id,
      name: '<img src=x onerror=alert(1)>',
      localPath: '/private/unrelated-file.mp4',
      videoUrl: 'https://unrelated.invalid/private.mp4',
      url: 'https://unrelated.invalid/other.mp4',
      media: 'image',
    })
    expect(native.convertFileSrc).toHaveBeenCalledExactlyOnceWith(id, 'plugin-media')
    const video = wrapper.get('video')
    expect(video.attributes('src')).toBe(`plugin-media://localhost/${id}`)
    expect(video.element.autoplay).toBe(true)
    expect(video.element.muted).toBe(true)
    expect(video.element.playsInline).toBe(true)
    expect(wrapper.find('img').exists()).toBe(false)
    expect(wrapper.text()).toContain('<img src=x onerror=alert(1)>')
    expect(wrapper.html()).not.toContain('/private/unrelated-file.mp4')
    expect(wrapper.html()).not.toContain('unrelated.invalid')
    expect(native.invoke).not.toHaveBeenCalled()
  })

  it.each([
    ['main', id],
    ['config', id],
    ['camera-video-legacy', id],
    ['plugin-video-other', id],
    [owner, ''],
    [owner, '../other'],
    [owner, id.toUpperCase()],
  ])(
    'rejects owner %s with identifier %s without falling back to a local asset',
    async (label, mediaId) => {
      const wrapper = await player(
        {
          pluginMedia: mediaId,
          localPath: '/private/unrelated-file.mp4',
          videoUrl: 'https://unrelated.invalid/private.mp4',
        },
        label
      )
      expect(wrapper.text()).toContain('This video window is unavailable.')
      expect(wrapper.find('video').exists()).toBe(false)
      expect(wrapper.find('img').exists()).toBe(false)
      expect(native.convertFileSrc).not.toHaveBeenCalled()
      expect(native.invoke).not.toHaveBeenCalled()
      expect(wrapper.html()).not.toContain('unrelated.invalid')
      expect(wrapper.html()).not.toContain('/private/unrelated-file.mp4')
    }
  )

  it('requires an identifier even when the native window has a plugin owner label', async () => {
    const wrapper = await player({ localPath: '/private/legacy.mp4', media: 'image' })
    expect(wrapper.text()).toContain('This video window is unavailable.')
    expect(wrapper.find('video').exists()).toBe(false)
    expect(wrapper.find('img').exists()).toBe(false)
    expect(native.convertFileSrc).not.toHaveBeenCalled()
  })

  it.each(['ended', 'click'])('uses only the owned native close command on %s', async (event) => {
    const wrapper = await player({ pluginMedia: id })
    const target =
      event === 'ended' ? wrapper.get('video') : wrapper.get('button[aria-label="Close"]')
    await target.trigger(event)
    await flushPromises()
    expect(native.invoke).toHaveBeenCalledExactlyOnceWith('close_plugin_video_window')
    expect(native.close).not.toHaveBeenCalled()
  })

  it('does not fall back to generic window close when the owned close command fails', async () => {
    native.invoke.mockRejectedValue(new Error('private native error https://private.invalid'))
    const wrapper = await player({ pluginMedia: id })
    await wrapper.get('button[aria-label="Close"]').trigger('click')
    await flushPromises()
    expect(native.invoke).toHaveBeenCalledExactlyOnceWith('close_plugin_video_window')
    expect(native.close).not.toHaveBeenCalled()
    expect(native.warn).toHaveBeenCalledExactlyOnceWith('Cannot close owned plugin video window')
    expect(wrapper.html()).not.toContain('private.invalid')
  })

  it('uses only the owned drag command for the title strip and no generic drag region', async () => {
    const wrapper = await player({ pluginMedia: id, name: 'Front' })
    expect(wrapper.find('[data-tauri-drag-region]').exists()).toBe(false)
    const titleStrip = wrapper.get('span').element.parentElement
    expect(titleStrip).not.toBeNull()
    titleStrip?.dispatchEvent(new MouseEvent('mousedown', { button: 0, bubbles: true }))
    await flushPromises()
    expect(native.invoke).toHaveBeenCalledExactlyOnceWith('drag_plugin_video_window')
    expect(native.close).not.toHaveBeenCalled()
    native.invoke.mockClear()
    titleStrip?.dispatchEvent(new MouseEvent('mousedown', { button: 2, bubbles: true }))
    await flushPromises()
    expect(native.invoke).not.toHaveBeenCalled()
    await wrapper.get('button[aria-label="Close"]').trigger('mousedown', { button: 0 })
    await flushPromises()
    expect(native.invoke).not.toHaveBeenCalled()
  })

  it('displays generic download failures without resolving media or exposing the source', async () => {
    const source = 'https://private.invalid/clip.mp4?token=private-token'
    const wrapper = await player({
      pluginMedia: id,
      name: 'Front',
      error: source,
      localPath: '/private/unrelated.mp4',
      videoUrl: source,
    })
    expect(wrapper.text()).toContain('Failed to download camera clip.')
    expect(wrapper.find('video').exists()).toBe(false)
    expect(wrapper.find('img').exists()).toBe(false)
    expect(native.convertFileSrc).not.toHaveBeenCalled()
    expect(native.invoke).not.toHaveBeenCalled()
    expect(wrapper.html()).not.toContain('private.invalid')
    expect(wrapper.html()).not.toContain('private-token')
    expect(native.warn).toHaveBeenCalledExactlyOnceWith(
      'Camera clip error:',
      'Failed to download camera clip.'
    )
  })

  it('clears a failed local playback without logging the opaque media URL', async () => {
    const wrapper = await player({ pluginMedia: id })
    await wrapper.get('video').trigger('error')
    await flushPromises()
    expect(wrapper.find('video').exists()).toBe(false)
    expect(wrapper.text()).toContain('Failed to play camera clip.')
    expect(native.convertFileSrc).toHaveBeenCalledTimes(1)
    expect(native.invoke).not.toHaveBeenCalled()
    expect(native.warn).toHaveBeenCalledExactlyOnceWith('Plugin video playback failed')
  })

  it('displays an owned still for twelve seconds after load and closes through its native owner', async () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
    const wrapper = await player({ pluginMedia: id, pluginMediaKind: 'image', name: 'Front' })
    expect(native.convertFileSrc).toHaveBeenCalledExactlyOnceWith(id, 'plugin-media')
    expect(wrapper.find('video').exists()).toBe(false)
    expect(wrapper.get('img').attributes('src')).toBe(`plugin-media://localhost/${id}`)
    await vi.advanceTimersByTimeAsync(12000)
    expect(native.invoke).not.toHaveBeenCalled()
    await wrapper.get('img').trigger('load')
    await vi.advanceTimersByTimeAsync(6000)
    await wrapper.get('img').trigger('load')
    await vi.advanceTimersByTimeAsync(5999)
    expect(native.invoke).not.toHaveBeenCalled()
    await vi.advanceTimersByTimeAsync(1)
    expect(native.invoke).toHaveBeenCalledExactlyOnceWith('close_plugin_video_window')
    expect(native.close).not.toHaveBeenCalled()
  })

  it.each(['close', 'unmount', 'error'])(
    'clears the owned still timer on %s without a delayed second close',
    async (action) => {
      vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
      const wrapper = await player({ pluginMedia: id, pluginMediaKind: 'image' })
      await wrapper.get('img').trigger('load')
      if (action === 'close') {
        await wrapper.get('button[aria-label="Close"]').trigger('click')
        expect(native.invoke).toHaveBeenCalledExactlyOnceWith('close_plugin_video_window')
        native.invoke.mockClear()
      } else if (action === 'unmount') {
        wrapper.unmount()
        mounted.splice(mounted.indexOf(wrapper), 1)
      } else {
        await wrapper.get('img').trigger('error')
        expect(wrapper.find('img').exists()).toBe(false)
        expect(wrapper.text()).toContain('Failed to display camera snapshot.')
        expect(native.warn).toHaveBeenCalledExactlyOnceWith('Plugin image display failed')
      }
      await vi.advanceTimersByTimeAsync(12000)
      expect(native.invoke).not.toHaveBeenCalled()
      expect(native.close).not.toHaveBeenCalled()
    }
  )

  it('rejects unknown media selectors without resolving or rendering content', async () => {
    const wrapper = await player({ pluginMedia: id, pluginMediaKind: 'svg' })
    expect(wrapper.text()).toContain('This video window is unavailable.')
    expect(wrapper.find('video').exists()).toBe(false)
    expect(wrapper.find('img').exists()).toBe(false)
    expect(native.convertFileSrc).not.toHaveBeenCalled()
  })

  it('renders a generic image download failure without resolving an owned handle', async () => {
    const wrapper = await player({ pluginMedia: id, pluginMediaKind: 'image', error: 'download' })
    expect(wrapper.text()).toContain('Failed to download camera snapshot.')
    expect(wrapper.find('video').exists()).toBe(false)
    expect(wrapper.find('img').exists()).toBe(false)
    expect(native.convertFileSrc).not.toHaveBeenCalled()
    expect(native.invoke).not.toHaveBeenCalled()
  })

  it('rejects legacy local file routes after provider extraction', async () => {
    const wrapper = await player(
      { localPath: '/private/legacy.jpg', media: 'image' },
      'camera-video-legacy'
    )
    expect(wrapper.text()).toContain('This video window is unavailable.')
    expect(wrapper.find('img, video').exists()).toBe(false)
    expect(native.convertFileSrc).not.toHaveBeenCalled()
    expect(native.invoke).not.toHaveBeenCalled()
  })
})
