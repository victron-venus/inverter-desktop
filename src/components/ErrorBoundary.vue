<template>
  <slot v-if="!hasError" />
  <div v-else class="flex flex-col items-center justify-center h-screen app-shell p-6">
    <div class="classic-card p-6 w-full max-w-md flex flex-col items-center text-center shadow-lg">
      <div class="text-4xl mb-3 opacity-90">⚠️</div>
      <h1 class="text-lg font-semibold tracking-tight text-main mb-1">Something went wrong</h1>
      <p class="text-muted text-[12px] leading-relaxed max-w-sm mb-4">
        {{ errorMessage || 'An unexpected error occurred. The app will attempt to recover.' }}
      </p>
      <div v-if="showRetry" class="flex gap-2 w-full">
        <UiButton variant="primary" size="lg" class="flex-1" @click="resetError"
          >Try Again</UiButton
        >
        <UiButton size="lg" class="flex-1" @click="reloadApp">Reload App</UiButton>
      </div>
      <details v-if="errorStack" class="mt-5 w-full text-left">
        <summary class="cursor-pointer text-[11px] font-semibold text-muted hover:text-main">
          Error Details
        </summary>
        <pre
          class="mt-2 p-3 classic-inset !rounded-lg text-[10px] overflow-auto max-h-64 text-consumption font-mono"
          >{{ errorStack }}</pre>
      </details>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onErrorCaptured, onMounted } from 'vue'
import UiButton from './UiButton.vue'
import { logger } from '../logger'

const hasError = ref(false)
const errorMessage = ref('')
const errorStack = ref('')
const showRetry = ref(false)

function resetError() {
  hasError.value = false
  errorMessage.value = ''
  errorStack.value = ''
}

function reloadApp() {
  globalThis.location.reload()
}

onMounted(() => {
  showRetry.value = true
})

onErrorCaptured((err, _instance, info) => {
  hasError.value = true
  errorMessage.value = err?.message || String(err)
  errorStack.value = `${info}\n${err?.stack || ''}`
  logger.error('ErrorBoundary caught:', err, info)
  return false
})
</script>
