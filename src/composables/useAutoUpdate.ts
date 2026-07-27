import type { Update } from '@tauri-apps/plugin-updater'
import { check } from '@tauri-apps/plugin-updater'
import { logger } from '../logger'

export async function checkForUpdates(): Promise<void> {
  try {
    const update: Update | null = await check()
    if (update?.available) {
      logger.info('Update available:', update.version)

      // Notify user about update
      if (window.confirm(`New version ${update.version} available. Download and install now?`)) {
        await installUpdate(update)
      }
    } else {
      logger.info('No updates available')
    }
  } catch (error) {
    logger.error('Failed to check for updates:', error)
  }
}

async function installUpdate(update: Update): Promise<void> {
  try {
    logger.info('Downloading update...')
    await update.download()
    logger.info('Installing update...')
    await update.install()
    logger.info('Update installed. Restarting...')
  } catch (error) {
    logger.error('Failed to install update:', error)
  }
}

export async function checkForUpdatesSilent(): Promise<void> {
  try {
    const update: Update | null = await check()
    if (update?.available) {
      logger.info('Update available:', update.version)
      // Could emit an event to notify UI
    }
  } catch (error) {
    logger.error('Silent update check failed:', error)
  }
}
