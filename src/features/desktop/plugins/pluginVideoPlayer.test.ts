import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import CameraVideo from '../../../CameraVideo.vue'

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
  globalThis.history.replaceState({}, '', initialLocation)
})

describe('native-owned plugin video player', () => {
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
})
