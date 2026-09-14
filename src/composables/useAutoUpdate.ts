import { openUrl } from '@tauri-apps/plugin-opener'
import { logger } from '../logger'

export const RELEASE_DOWNLOAD_URL =
  'https://github.com/victron-venus/inverter-desktop/releases/latest'

export async function checkForUpdates(): Promise<void> {
  try {
    await openUrl(RELEASE_DOWNLOAD_URL)
  } catch (error) {
    logger.error('Failed to open release downloads:', error)
    window.alert(
      `Could not open the downloads page. Download and install updates manually from:\n${RELEASE_DOWNLOAD_URL}`
    )
  }
}
