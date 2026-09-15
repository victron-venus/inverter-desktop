<template>
  <ErrorBoundary>
    <div class="h-screen w-screen bg-black flex flex-col select-none overflow-hidden">
      <div
        class="flex items-center justify-between px-3 py-2 bg-gradient-to-b from-black/90 to-transparent absolute top-0 left-0 right-0 z-50 pointer-events-auto"
        role="toolbar"
        aria-label="Camera window controls"
        @mousedown.left="dragOwnedWindow"
      >
        <!-- Tauri 2 drag region: title strip only so the close button stays clickable -->
        <div
          :data-tauri-drag-region="isPluginMedia ? undefined : true"
          class="flex items-center gap-2 min-w-0 flex-1 h-full cursor-default"
        >
          <div
            class="w-1.5 h-1.5 rounded-full bg-red-500 animate-pulse shrink-0 pointer-events-none"
          ></div>
          <span
            class="text-[11px] font-semibold text-white tracking-tight truncate pointer-events-none"
          >
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
        v-if="videoUrl && mediaKind === 'video'"
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

      <img
        v-else-if="videoUrl && mediaKind === 'image'"
        class="w-full h-full object-contain bg-black"
        :src="videoUrl"
        alt="Camera snapshot"
        @error="onVideoError"
      />

      <div
        v-else
        class="flex-1 min-h-0 flex flex-col overflow-y-auto text-white/70 text-[13px] mt-12 px-4 pb-4 text-center"
      >
        <p class="my-auto shrink-0 whitespace-pre-wrap [overflow-wrap:anywhere]">
          {{ errorMessage || 'Waiting for camera clip…' }}
        </p>
      </div>
    </div>
  </ErrorBoundary>
</template>

<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue'
import { X } from '@lucide/vue'
import { convertFileSrc, invoke } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'
import ErrorBoundary from './components/ErrorBoundary.vue'
import { logger } from './logger'
import { pluginVideoRoute } from './features/desktop/plugins/pluginVideoRoute'

const videoUrl = ref('')
const cameraName = ref('Camera')
const errorMessage = ref('')
const mediaKind = ref<'video' | 'image'>('video')
const videoEl = ref<HTMLVideoElement | null>(null)
const isPluginMedia = ref(false)
let imageCloseTimer: ReturnType<typeof setTimeout> | null = null

function setName(name?: string | null) {
  cameraName.value = name?.trim() || 'Camera'
}

function clearImageCloseTimer() {
  if (imageCloseTimer) {
    clearTimeout(imageCloseTimer)
    imageCloseTimer = null
  }
}

function loadLocalClip(localPath: string, name?: string | null, media?: string | null) {
  errorMessage.value = ''
  setName(name)
  mediaKind.value = media === 'image' ? 'image' : 'video'
  // Serve via Tauri asset protocol — raw http:// fails in WKWebView (mixed content).
  videoUrl.value = convertFileSrc(localPath)
  if (mediaKind.value === 'image') {
    clearImageCloseTimer()
    // Stills have no @ended — auto-close after a short view.
    imageCloseTimer = setTimeout(() => {
      void closeWindow()
    }, 12000)
    return
  }
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
  if (isPluginMedia.value) logger.warn('Plugin video playback failed')
  else logger.warn('Camera video playback error for', videoUrl.value)
  videoUrl.value = ''
}

async function closeWindow() {
  clearImageCloseTimer()
  if (isPluginMedia.value) {
    try {
      await invoke('close_plugin_video_window')
    } catch {
      logger.warn('Cannot close owned plugin video window')
    }
    return
  }
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

async function dragOwnedWindow(event: MouseEvent) {
  if (
    !isPluginMedia.value ||
    !(event.target instanceof Element) ||
    event.target.closest('button')
  ) {
    return
  }
  try {
    await invoke('drag_plugin_video_window')
  } catch {
    logger.warn('Cannot drag owned plugin video window')
  }
}

onMounted(() => {
  const params = new URLSearchParams(globalThis.location.search)
  let label = ''
  try {
    label = getCurrentWindow().label
  } catch {
    // Browser-only previews have no native window or plugin media authority.
  }
  isPluginMedia.value = label.startsWith('plugin-video-') || params.has('pluginMedia')
  if (isPluginMedia.value) {
    const route = pluginVideoRoute(globalThis.location.search, label)
    if (!route) {
      showError('This video window is unavailable.')
    } else if (route.failed) {
      showError('Failed to download camera clip.', route.name)
    } else {
      setName(route.name)
      mediaKind.value = 'video'
      videoUrl.value = convertFileSrc(route.id, 'plugin-media')
    }
    return
  }
  const name = params.get('name')
  const error = params.get('error')
  const localPath = params.get('localPath')
  const media = params.get('media')
  if (error) {
    showError(error, name)
  } else if (localPath) {
    loadLocalClip(localPath, name, media)
  } else {
    showError('No camera clip provided.', name)
  }
})

onUnmounted(() => {
  clearImageCloseTimer()
})
</script>
