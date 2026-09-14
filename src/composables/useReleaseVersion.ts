import { onMounted, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { logger } from '../logger'

// Both the main status bar and About read the same embedded identity, including offline builds.
export function useReleaseVersion() {
  const appVersion = ref('…')
  onMounted(async () => {
    try {
      const info = await invoke<{ version: string }>('get_release_info')
      if (!info.version) throw new Error('Missing embedded release version')
      appVersion.value = info.version
    } catch (error) {
      logger.error('Failed to read embedded release version:', error)
      appVersion.value = 'unknown'
    }
  })
  return appVersion
}
