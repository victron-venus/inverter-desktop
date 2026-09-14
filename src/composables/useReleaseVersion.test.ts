import { defineComponent } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { invoke } from '@tauri-apps/api/core'
import About from '../About.vue'
import { useReleaseVersion } from './useReleaseVersion'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('../logger', () => ({ logger: { error: vi.fn() } }))

const Status = defineComponent({
  setup: () => ({ version: useReleaseVersion() }),
  template: '<span>{{ version }}</span>',
})

describe('embedded release version', () => {
  beforeEach(() => vi.resetAllMocks())

  it.each(['2.5.42-beta.3', '2.5.42-rc.1', '2.5.42'])(
    'shows %s consistently in the shared status source and About',
    async (version) => {
      vi.mocked(invoke).mockResolvedValue({ version })
      const status = mount(Status)
      const about = mount(About)
      await flushPromises()
      expect(status.text()).toBe(version)
      expect(about.text()).toContain(`Version ${version}`)
      expect(invoke).toHaveBeenCalledWith('get_release_info')
      status.unmount()
      about.unmount()
    }
  )

  it('does not fabricate a stable version when IPC fails', async () => {
    vi.mocked(invoke).mockRejectedValue(new Error('IPC unavailable'))
    const about = mount(About)
    await flushPromises()
    expect(about.text()).toContain('Version unknown')
    expect(about.text()).not.toContain('1.1.2')
    about.unmount()
  })
})
