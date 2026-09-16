import { describe, expect, it } from 'vitest'
import { pluginVideoRoute } from './pluginVideoRoute'

const id = 'acdb88b6-453a-4f88-8d52-5b902441fe4c'
const label = `plugin-video-${id}`

describe('owned plugin video route', () => {
  it('uses an opaque handle and ignores legacy paths and remote URLs', () => {
    expect(
      pluginVideoRoute(
        `?pluginMedia=${id}&name=Front&localPath=/private/other&videoUrl=https://other.invalid`,
        label
      )
    ).toEqual({ id, mediaKind: 'video', name: 'Front', failed: false })
  })

  it('requires the exact native owner and canonical identifier', () => {
    for (const owner of ['main', 'config', 'camera-video-1', 'plugin-video-other']) {
      expect(pluginVideoRoute(`?pluginMedia=${id}`, owner)).toBeNull()
    }
    for (const value of ['../other', `${id}/other`, id.toUpperCase(), '']) {
      expect(pluginVideoRoute(`?pluginMedia=${encodeURIComponent(value)}`, label)).toBeNull()
    }
  })

  it('exposes only a generic download failure flag', () => {
    expect(pluginVideoRoute(`?pluginMedia=${id}&error=download`, label)).toEqual({
      id,
      mediaKind: 'video',
      name: 'Camera',
      failed: true,
    })
  })

  it('accepts only the dedicated typed media selector and ignores legacy image flags', () => {
    expect(pluginVideoRoute(`?pluginMedia=${id}&pluginMediaKind=image`, label)).toEqual({
      id,
      mediaKind: 'image',
      name: 'Camera',
      failed: false,
    })
    expect(pluginVideoRoute(`?pluginMedia=${id}&media=image`, label)?.mediaKind).toBe('video')
    for (const kind of ['', 'svg', 'html', 'IMAGE', 'https://other.invalid/image.jpg']) {
      expect(pluginVideoRoute(`?pluginMedia=${id}&pluginMediaKind=${kind}`, label)).toBeNull()
    }
  })

  it('requires an explicit live kind and exact preview owner without accepting a route URL', () => {
    const live = `plugin-preview-${id}`
    expect(
      pluginVideoRoute(`?pluginMedia=${id}&pluginMediaKind=live&url=https://ignored.invalid`, live)
    ).toEqual({ id, mediaKind: 'live', name: 'Camera', failed: false })
    for (const kind of ['video', 'image', '']) {
      expect(pluginVideoRoute(`?pluginMedia=${id}&pluginMediaKind=${kind}`, live)).toBeNull()
    }
    for (const owner of [
      label,
      'main',
      `plugin-live-${id}`,
      `plugin-preview-${id.toUpperCase()}`,
    ]) {
      expect(pluginVideoRoute(`?pluginMedia=${id}&pluginMediaKind=live`, owner)).toBeNull()
    }
    expect(pluginVideoRoute(`?pluginMedia=${id}&live=1`, live)).toBeNull()
  })
})
