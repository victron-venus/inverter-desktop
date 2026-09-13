<template>
  <slot v-if="unlocked" />
  <AuthScreen v-else-if="ready && !error" @authenticated="refresh" />
  <div v-else class="min-h-screen flex flex-col items-center justify-center gap-3 p-6">
    <p>{{ error || 'Opening secure settings…' }}</p>
    <button type="button" v-if="error" class="classic-input px-4" @click="refresh">Retry</button>
  </div>
</template>

<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { onMounted, onUnmounted, ref } from 'vue'
import AuthScreen from './AuthScreen.vue'

const unlocked = ref(false)
const ready = ref(false)
const error = ref('')
let alive = true
let request = 0
let unlisten: (() => void) | undefined
let listenerPromise: Promise<void> | undefined
let timer: ReturnType<typeof setInterval> | undefined

async function ensureListener() {
  if (unlisten) return
  if (!listenerPromise) {
    listenerPromise = listen('auth-state-changed', () => {
      // Revoke the rendered slot immediately while checking the new policy/session.
      unlocked.value = false
      ready.value = false
      void refresh()
    })
      .then((stop) => {
        if (alive) unlisten = stop
        else stop()
      })
      .finally(() => {
        listenerPromise = undefined
      })
  }
  await listenerPromise
}

async function refresh() {
  if (!alive) return
  const current = ++request
  try {
    // Subscription failure is retryable and must never open a protected window.
    await ensureListener()
    if (!alive || current !== request) return
    const status = await invoke<{ unlocked: boolean }>('auth_status')
    if (!alive || current !== request) return
    unlocked.value = status.unlocked === true
    error.value = ''
  } catch (e) {
    if (!alive || current !== request) return
    unlocked.value = false
    error.value = `Unable to open settings: ${String(e)}`
  } finally {
    if (alive && current === request) ready.value = true
  }
}

onMounted(() => {
  void refresh()
  timer = setInterval(refresh, 15000)
})

onUnmounted(() => {
  alive = false
  request += 1
  unlisten?.()
  if (timer) clearInterval(timer)
})
</script>
