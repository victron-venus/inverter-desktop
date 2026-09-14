import { afterEach, describe, expect, it, vi } from 'vitest'
import { openUrl } from '@tauri-apps/plugin-opener'
import { checkForUpdates, RELEASE_DOWNLOAD_URL } from './useAutoUpdate'

vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }))
vi.mock('../logger', () => ({ logger: { error: vi.fn() } }))

afterEach(() => vi.restoreAllMocks())

describe('manual update downloads', () => {
  it('opens the release page without downloading or installing a package', async () => {
    vi.mocked(openUrl).mockResolvedValueOnce()
    await checkForUpdates()
    expect(openUrl).toHaveBeenCalledWith(RELEASE_DOWNLOAD_URL)
  })

  it('shows the manual download URL when the browser cannot open', async () => {
    vi.mocked(openUrl).mockRejectedValueOnce(new Error('No browser available'))
    const alert = vi.spyOn(window, 'alert').mockImplementation(() => {})
    await checkForUpdates()
    expect(alert).toHaveBeenCalledWith(expect.stringContaining(RELEASE_DOWNLOAD_URL))
  })
})
