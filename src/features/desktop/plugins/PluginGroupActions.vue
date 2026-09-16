<template>
  <UiButton
    v-for="group in groups"
    :key="group.id"
    class="min-w-[22px] !px-1.5 shrink-0"
    toggle
    :active="group.enabled"
    :disabled="!available || busy.has(group.id)"
    :title="group.title"
    :aria-label="group.title"
    @click="toggle(group)"
  >
    <Camera v-if="group.icon === 'camera' && group.enabled" :size="11" /><CameraOff
      v-else-if="group.icon === 'camera'"
      :size="11"
    /><Puzzle v-else :size="11" />
  </UiButton>
</template>
<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { Camera, CameraOff, Puzzle } from '@lucide/vue'
import UiButton from '../../../components/UiButton.vue'
import type { PluginGroup } from './types'
const emit = defineEmits<{ error: [message: string] }>()
const groups = ref<PluginGroup[]>([])
const available = ref(false)
const busy = ref(new Set<string>())
let active = false
let generation = 0
let revision = 0
let authEpoch = 0
let listeners: Array<() => void> = []
async function refresh() {
  const current = ++revision
  const session = generation
  try {
    const status = await invoke<{ unlocked: boolean }>('auth_status')
    if (!active || generation !== session || current !== revision) return
    if (!status?.unlocked) {
      groups.value = []
      available.value = false
      return
    }
    const value = await invoke<PluginGroup[]>('get_plugin_groups')
    if (!active || generation !== session || current !== revision) return
    groups.value = value
    available.value = true
  } catch {
    if (active && generation === session && current === revision) available.value = false
  }
}
async function toggle(group: PluginGroup) {
  if (!active || !available.value || busy.value.has(group.id)) return
  const session = generation
  const epoch = authEpoch
  busy.value.add(group.id)
  try {
    await invoke('set_plugin_group_enabled', { groupId: group.id, enabled: !group.enabled })
  } catch {
    if (active && generation === session && authEpoch === epoch)
      emit('error', 'Could not update plugin monitoring. Reload settings and try again.')
  } finally {
    if (active && generation === session && authEpoch === epoch) {
      busy.value.delete(group.id)
      void refresh()
    }
  }
}
onMounted(async () => {
  active = true
  const session = ++generation
  for (const name of ['plugin-host-update', 'auth-state-changed']) {
    try {
      const stop = await listen(name, () => {
        if (!active || generation !== session) return
        if (name === 'auth-state-changed') {
          authEpoch += 1
          busy.value.clear()
          groups.value = []
          available.value = false
          revision += 1
        }
        void refresh()
      })
      if (!active || generation !== session) {
        stop()
        return
      }
      listeners.push(stop)
    } catch {
      return
    }
  }
  await refresh()
})
onUnmounted(() => {
  active = false
  generation += 1
  for (const stop of listeners) stop()
  listeners = []
  groups.value = []
  available.value = false
  busy.value.clear()
})
</script>
