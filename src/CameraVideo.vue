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
import { convertFileSrc } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import ErrorBoundary from './components/ErrorBoundary.vue'
import { logger } from './logger'

type CameraClipPayload = {
  local_path?: string
  agent_name?: string
  error?: string
  loading?: boolean
}

const videoUrl = ref('')
const cameraName = ref('Camera')
const errorMessage = ref('')
const videoEl = ref<HTMLVideoElement | null>(null)
let unlistenUpdate: UnlistenFn | null = null

function setName(name?: string) {
  cameraName.value = (name && name.trim()) || 'Camera'
}

function loadLocalClip(localPath: string, name?: string) {
  errorMessage.value = ''
  setName(name)
  // Serve via Tauri asset protocol — raw http:// fails in WKWebView (mixed content).
  videoUrl.value = convertFileSrc(localPath)
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

function showError(message: string, name?: string) {
  videoUrl.value = ''
  setName(name)
  errorMessage.value = message
  logger.warn('Camera clip error:', message)
}

function showLoading(name?: string) {
  videoUrl.value = ''
  setName(name)
  errorMessage.value = 'Downloading camera clip…'
}

function onVideoError() {
  errorMessage.value = 'Failed to play camera clip. Local file may be missing or unsupported.'
  logger.warn('Camera video playback error for', videoUrl.value)
  videoUrl.value = ''
}

async function closeWindow() {
  try {
    await getCurrentWindow().close()
  } catch (e) {
    logger.warn('Failed to close camera video window:', e)
  }
}

function applyPayload(payload: CameraClipPayload | null | undefined) {
  if (!payload) return
  if (payload.loading) {
    showLoading(payload.agent_name)
    return
  }
  if (payload.error) {
    showError(payload.error, payload.agent_name)
    return
  }
  if (payload.local_path) {
    loadLocalClip(payload.local_path, payload.agent_name)
  }
}

onMounted(async () => {
  const params = new URLSearchParams(globalThis.location.search)
  const name = params.get('name') || undefined
  const error = params.get('error')
  const localPath = params.get('localPath')
  if (error) {
    showError(error, name)
  } else if (localPath) {
    loadLocalClip(localPath, name)
  } else {
    showLoading(name)
  }

  try {
    unlistenUpdate = await listen<CameraClipPayload>('camera-clip-update', (event) => {
      applyPayload(event.payload)
    })
  } catch (e) {
    logger.warn('Failed to listen for camera-clip-update:', e)
  }
})

onUnmounted(() => {
  if (unlistenUpdate) unlistenUpdate()
})
</script>
