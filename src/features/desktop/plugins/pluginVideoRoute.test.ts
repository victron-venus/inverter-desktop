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
    ).toEqual({ id, name: 'Front', failed: false })
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
      name: 'Camera',
      failed: true,
    })
  })
})
