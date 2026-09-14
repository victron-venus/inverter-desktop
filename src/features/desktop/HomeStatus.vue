<template>
  <HoverTip v-if="haEnabled" :text="$t('status.tipHa')" class="flex items-center gap-1.5">
    <div class="status-dot" :class="{ 'status-dot-on': haConnected }"></div>
    <span>{{ $t('status.ha') }}</span>
  </HoverTip>

  <span v-if="haEnabled" class="soft-divider"></span>
</template>
<script setup lang="ts">
import { inject } from 'vue'
import { useI18n } from 'vue-i18n'
import HoverTip from '../../components/HoverTip.vue'
import type { useHA } from '../../composables/useHA'
const { haEnabled, haConnected } = inject<ReturnType<typeof useHA>>('desktop-home')!
const { t: $t } = useI18n()
</script>
