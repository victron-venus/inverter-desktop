<template>
  <ErrorBoundary>
    <div class="h-screen w-screen bg-black flex flex-col select-none overflow-hidden">
      <div
        class="flex items-center justify-between px-3 py-2 bg-gradient-to-b from-black/90 to-transparent absolute top-0 left-0 right-0 z-10"
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
        controls
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
import { onMounted, onUnmounted, ref } from 'vue'
import { X } from '@lucide/vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import ErrorBoundary from './components/ErrorBoundary.vue'
import { logger } from './logger'

const videoUrl = ref('')
const cameraName = ref('Camera')
const errorMessage = ref('')
const videoEl = ref<HTMLVideoElement | null>(null)
let unlistenUpdate: UnlistenFn | null = null

function loadClip(url: string, name?: string) {
  errorMessage.value = ''
  cameraName.value = (name && name.trim()) || 'Camera'
  videoUrl.value = url
  // Force reload when the same element gets a new src
  requestAnimationFrame(() => {
    const el = videoEl.value
    if (el) {
      el.load()
      el.play().catch(() => {
        // Autoplay may be blocked until user interaction; controls remain available.
      })
    }
  })
}

function onVideoError() {
  errorMessage.value = 'Failed to play camera clip. Check network / CSP media permissions.'
  logger.warn('Camera video playback error for', videoUrl.value)
}

async function closeWindow() {
  try {
    await getCurrentWindow().close()
  } catch (e) {
    logger.warn('Failed to close camera video window:', e)
  }
}

onMounted(async () => {
  const params = new URLSearchParams(globalThis.location.search)
  const url = params.get('url')
  const name = params.get('name') || undefined
  if (url) {
    loadClip(url, name)
  }

  try {
    unlistenUpdate = await listen<{ video_url: string; agent_name?: string }>(
      'camera-clip-update',
      (event) => {
        if (event.payload?.video_url) {
          loadClip(event.payload.video_url, event.payload.agent_name)
        }
      }
    )
  } catch (e) {
    logger.warn('Failed to listen for camera-clip-update:', e)
  }
})

onUnmounted(() => {
  if (unlistenUpdate) unlistenUpdate()
})
</script>
