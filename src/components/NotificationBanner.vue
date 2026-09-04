<template>
  <div v-if="bannerNotifications.length > 0" class="flex flex-col gap-1">
    <div
      v-for="banner in bannerNotifications"
      :key="banner.id"
      class="flex items-center gap-2 rounded-lg border px-3 py-1.5 text-[12px] leading-tight"
      :class="levelClasses[banner.level] ?? levelClasses.info"
    >
      <AlertOctagon v-if="banner.level === 'alarm'" :size="15" class="shrink-0" />
      <TriangleAlert v-else-if="banner.level === 'warning'" :size="15" class="shrink-0" />
      <Info v-else :size="15" class="shrink-0" />
      <div class="flex items-center gap-2 min-w-0 flex-1">
        <span class="font-semibold shrink-0">{{ banner.title }}</span>
        <span v-if="banner.body" class="opacity-80 truncate min-w-0">{{ banner.body }}</span>
      </div>
      <span v-if="banner.ts" class="text-[10px] opacity-60 shrink-0 whitespace-nowrap tabular-nums">
        {{ formatTimestamp(banner.ts) }}
      </span>
      <button
        type="button"
        class="shrink-0 rounded p-0.5 opacity-50 transition-opacity hover:opacity-100 cursor-pointer"
        :title="$t('notifications.dismiss')"
        @click="dismissBanner(banner.id)"
      >
        <X :size="14" />
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { AlertOctagon, Info, TriangleAlert, X } from '@lucide/vue'
import { bannerNotifications, dismissBanner } from '../composables/useInverterState'
import { formatTimestamp } from '../utils'

const levelClasses: Record<string, string> = {
  info: 'bg-blue-500/10 text-blue-700 dark:text-blue-300 border-blue-500/30',
  warning: 'bg-amber-500/10 text-amber-700 dark:text-amber-300 border-amber-500/30',
  alarm: 'bg-red-500/10 text-red-700 dark:text-red-300 border-red-500/30',
}
</script>
