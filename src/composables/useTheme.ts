import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { getAppConfig } from '../config'
import { logger } from '../logger'

export function useTheme() {
  const isDark = ref(localStorage.getItem('theme') === 'dark')
  
  // Initial sync on module load
  const syncTheme = (dark: boolean) => {
    document.documentElement.classList.toggle('dark', dark)
    document.body.classList.toggle('dark', dark)
  }
  
  syncTheme(isDark.value)

  async function toggleTheme() {
    isDark.value = !isDark.value
    syncTheme(isDark.value)
    const scheme = isDark.value ? 'dark' : 'light'
    localStorage.setItem('theme', scheme)
    
    try {
      const cfg = await getAppConfig()
      cfg.color_scheme = scheme
      await invoke('save_config', { config: cfg })
    } catch (e) {
      logger.error('Failed to sync theme to config:', e)
    }
  }

  return { isDark, toggleTheme }
}
