<template>
  <ErrorBoundary>
    <div class="h-screen w-screen bg-black flex flex-col select-none overflow-hidden">
      <div
        class="flex items-center justify-between px-3 py-2 bg-gradient-to-b from-black/90 to-transparent absolute top-0 left-0 right-0 z-50 pointer-events-auto"
      >
        <div class="flex items-center gap-2 min-w-0">
          <div class="w-1.5 h-1.5 rounded-full bg-red-500 animate-pulse shrink-0"></div>
          <span class="text-[11px] font-semibold text-white tracking-tight truncate">
            {{ cameraName }}
          </span>
        </div>
        <button
          type="button"
          class="p-1.5 rounded-full bg-white/10 text-white hover:bg-white/20 transition-colors shrink-0"
          aria-label="Close"
          @click="closeWindow"
        >
          <X :size="18" />
        </button>
      </div>

      <video
        v-if="videoUrl"
        ref="videoEl"
        autoplay
        muted
        playsinline
        class="w-full h-full object-contain bg-black"
        :src="videoUrl"
        @ended="closeWindow"
        @error="onVideoError"
      >
        <track kind="captions" />
        Your browser does not support the video tag.
      </video>

      <div
        v-else
        class="flex-1 flex items-center justify-center text-white/70 text-[13px] px-4 text-center"
      >
        {{ errorMessage || 'Waiting for camera clip…' }}
      </div>
    </div>
  </ErrorBoundary>
</template>

<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { X } from '@lucide/vue'
import { convertFileSrc, invoke } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'
import ErrorBoundary from './components/ErrorBoundary.vue'
import { logger } from './logger'

const videoUrl = ref('')
const cameraName = ref('Camera')
const errorMessage = ref('')
const videoEl = ref<HTMLVideoElement | null>(null)

function setName(name?: string | null) {
  cameraName.value = (name && name.trim()) || 'Camera'
}

function loadLocalClip(localPath: string, name?: string | null) {
  errorMessage.value = ''
  setName(name)
  // Serve via Tauri asset protocol — raw http:// fails in WKWebView (mixed content).
  videoUrl.value = convertFileSrc(localPath)
  requestAnimationFrame(() => {
    const el = videoEl.value
    if (el) {
      el.muted = true
      el.volume = 0
      el.load()
      el.play().catch(() => {
        // Autoplay may be blocked until user interaction.
      })
    }
  })
}

function showError(message: string, name?: string | null) {
  videoUrl.value = ''
  setName(name)
  errorMessage.value = message
  logger.warn('Camera clip error:', message)
}

function onVideoError() {
  errorMessage.value = 'Failed to play camera clip. Local file may be missing or unsupported.'
  logger.warn('Camera video playback error for', videoUrl.value)
  videoUrl.value = ''
}

async function closeWindow() {
  try {
    await getCurrentWindow().close()
    return
  } catch (e) {
    logger.warn('getCurrentWindow().close() failed, trying invoke:', e)
  }
  try {
    await invoke('close_camera_video_window')
  } catch (e) {
    logger.warn('Failed to close camera video window:', e)
  }
}

onMounted(() => {
  const params = new URLSearchParams(globalThis.location.search)
  const name = params.get('name')
  const error = params.get('error')
  const localPath = params.get('localPath')
  if (error) {
    showError(error, name)
  } else if (localPath) {
    loadLocalClip(localPath, name)
  } else {
    showError('No camera clip provided.', name)
  }
})
</script>
