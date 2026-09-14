import { computed, type Ref } from 'vue'
import type { AppConfig } from '../../config'

/** Each field edit emits the component model update without mutating its input object. */
export function configField<Key extends keyof AppConfig>(config: Ref<AppConfig>, key: Key) {
  return computed<AppConfig[Key]>({
    get: () => config.value[key],
    set: (value) => {
      config.value = { ...config.value, [key]: value }
    },
  })
}
