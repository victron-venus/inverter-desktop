<template>
  <div class="notification-bell relative">
    <button type="button" class="classic-btn relative" @click="showPanel = !showPanel">
      🔔
      <span
        v-if="unreadCount > 0"
        class="absolute -top-1 -right-1 w-4 h-4 rounded-full bg-red-500 text-white text-[9px] flex items-center justify-center font-bold"
      >
        {{ unreadCount > 9 ? '9+' : unreadCount }}
      </span>
    </button>

    <div
      v-if="showPanel"
      class="absolute bottom-full right-0 mb-2 w-72 max-h-80 overflow-y-auto classic-card z-50"
    >
      <div
        class="flex items-center justify-between px-2 py-1 border-b border-slate-200 dark:border-slate-700"
      >
        <span class="text-[10px] font-bold text-slate-500 uppercase">{{
          $t('notifications.title')
        }}</span>
        <div class="flex gap-1">
          <button type="button"
            v-if="notifications.length > 0"
            class="text-[9px] text-blue-500 hover:underline"
            @click="markAllRead"
          >
            {{ $t('notifications.markAllRead') }}
          </button>
          <button type="button"
            v-if="notifications.length > 0"
            class="text-[9px] text-red-500 hover:underline"
            @click="clearAll"
          >
            {{ $t('notifications.clear') }}
          </button>
        </div>
      </div>
      <div
        v-if="notifications.length === 0"
        class="px-2 py-3 text-[10px] text-slate-400 text-center"
      >
        {{ $t('notifications.noNotifications') }}
      </div>
      <div
        v-for="n in notifications"
        :key="n.id"
        class="px-2 py-1 border-b border-slate-100 dark:border-slate-800 last:border-0 hover:bg-slate-50 dark:hover:bg-slate-800/50 cursor-pointer"
        :class="{ 'opacity-50': n.read }"
        @click="markRead(n.id)"
      >
        <div class="flex items-start justify-between">
          <span class="text-[10px] font-bold text-slate-700 dark:text-slate-300">{{
            n.title
          }}</span>
          <span class="text-[8px] text-slate-400 whitespace-nowrap ml-1">
            {{ formatTime(n.timestamp) }}
          </span>
        </div>
        <div class="text-[9px] text-slate-500 truncate">{{ n.body }}</div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  notifications,
  unreadNotificationCount,
  markNotificationRead,
  markAllNotificationsRead,
  clearNotifications,
} from '../composables/useInverterState'

const { t: $t } = useI18n()
const showPanel = ref(false)
const unreadCount = computed(() => unreadNotificationCount())

function formatTime(ts: number): string {
  const d = new Date(ts)
  return `${d.getHours().toString().padStart(2, '0')}:${d.getMinutes().toString().padStart(2, '0')}`
}

function markRead(id: number) {
  markNotificationRead(id)
}

function markAllRead() {
  markAllNotificationsRead()
}

function clearAll() {
  clearNotifications()
}

function onClickOutside(e: MouseEvent) {
  const target = e.target as HTMLElement
  if (!target.closest('.notification-bell')) {
    showPanel.value = false
  }
}

onMounted(() => {
  document.addEventListener('click', onClickOutside)
})

onUnmounted(() => {
  document.removeEventListener('click', onClickOutside)
})
</script>
