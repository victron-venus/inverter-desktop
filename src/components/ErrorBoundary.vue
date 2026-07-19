<template>
  <slot v-if="!hasError" />
  <div
    v-else
    class="flex flex-col items-center justify-center h-screen bg-slate-900 text-white p-4"
  >
    <div class="text-6xl mb-4">⚠️</div>
    <h1 class="text-2xl font-bold mb-2">Something went wrong</h1>
    <p class="text-slate-400 text-center max-w-md mb-4">
      {{ errorMessage || 'An unexpected error occurred. The app will attempt to recover.' }}
    </p>
    <div v-if="showRetry" class="flex gap-2">
      <button
        type="button"
        @click="resetError"
        class="px-4 py-2 bg-blue-600 hover:bg-blue-500 rounded-lg transition-colors"
      >
        Try Again
      </button>
      <button
        type="button"
        @click="reloadApp"
        class="px-4 py-2 bg-slate-700 hover:bg-slate-600 rounded-lg transition-colors"
      >
        Reload App
      </button>
    </div>
    <details v-if="errorStack" class="mt-6 w-full max-w-2xl">
      <summary class="cursor-pointer text-slate-400 hover:text-white">Error Details</summary>
      <pre class="mt-2 p-3 bg-slate-800 rounded text-xs overflow-auto max-h-64 text-red-300">{{
        errorStack
      }}</pre>
    </details>
  </div>
</template>

<script setup lang="ts">
import { ref, onErrorCaptured, onMounted } from 'vue'
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

onErrorCaptured((err, instance, info) => {
  hasError.value = true
  errorMessage.value = err?.message || String(err)
  errorStack.value = `${info}\n${err?.stack || ''}`
  logger.error('ErrorBoundary caught:', err, info)
  return false // Prevent propagate
})
</script>
