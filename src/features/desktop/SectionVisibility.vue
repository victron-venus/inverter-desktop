<template>
  <label
    class="flex items-center gap-2 px-2 py-1.5 rounded-lg border border-black/[0.08] dark:border-white/[0.1] bg-white dark:bg-[#1c1c1e] cursor-pointer group hover:border-accent/40 transition-colors"
  >
    <input
      type="checkbox"
      v-model="fields.show_washer"
      class="rounded border-slate-300 text-accent focus:ring-accent"
    />
    <span class="text-[11px] font-bold text-main">Washer</span>
  </label>
  <label
    class="flex items-center gap-2 px-2 py-1.5 rounded-lg border border-black/[0.08] dark:border-white/[0.1] bg-white dark:bg-[#1c1c1e] cursor-pointer group hover:border-accent/40 transition-colors"
  >
    <input
      type="checkbox"
      v-model="fields.show_dryer"
      class="rounded border-slate-300 text-accent focus:ring-accent"
    />
    <span class="text-[11px] font-bold text-main">Dryer</span>
  </label>
  <label
    class="flex items-center gap-2 px-2 py-1.5 rounded-lg border border-black/[0.08] dark:border-white/[0.1] bg-white dark:bg-[#1c1c1e] cursor-pointer group hover:border-accent/40 transition-colors"
  >
    <input
      type="checkbox"
      v-model="fields.show_dishwasher"
      class="rounded border-slate-300 text-accent focus:ring-accent"
    />
    <span class="text-[11px] font-bold text-main">Dishwasher</span>
  </label>
  <label
    class="flex items-center gap-2 px-2 py-1.5 rounded-lg border border-black/[0.08] dark:border-white/[0.1] bg-white dark:bg-[#1c1c1e] cursor-pointer group hover:border-accent/40 transition-colors"
  >
    <input
      type="checkbox"
      v-model="fields.show_ha_sensors"
      class="rounded border-slate-300 text-accent focus:ring-accent"
    />
    <span class="text-[11px] font-bold text-main">{{ $t('config.sensors') }}</span>
  </label>
  <label
    class="flex items-center gap-2 px-2 py-1.5 rounded-lg border border-black/[0.08] dark:border-white/[0.1] bg-white dark:bg-[#1c1c1e] cursor-pointer group hover:border-accent/40 transition-colors"
  >
    <input
      type="checkbox"
      v-model="fields.show_ha_numbers"
      class="rounded border-slate-300 text-accent focus:ring-accent"
    />
    <span class="text-[11px] font-bold text-main">{{ $t('config.numbers') }}</span>
  </label>
  <label
    class="flex items-center gap-2 px-2 py-1.5 rounded-lg border border-black/[0.08] dark:border-white/[0.1] bg-white dark:bg-[#1c1c1e] cursor-pointer group hover:border-accent/40 transition-colors"
  >
    <input
      type="checkbox"
      v-model="fields.show_ha_covers"
      class="rounded border-slate-300 text-accent focus:ring-accent"
    />
    <span class="text-[11px] font-bold text-main">{{ $t('config.covers') }}</span>
  </label>
  <label
    class="flex items-center gap-2 px-2 py-1.5 rounded-lg border border-black/[0.08] dark:border-white/[0.1] bg-white dark:bg-[#1c1c1e] cursor-pointer group hover:border-accent/40 transition-colors"
  >
    <input
      type="checkbox"
      v-model="fields.show_ha_media"
      class="rounded border-slate-300 text-accent focus:ring-accent"
    />
    <span class="text-[11px] font-bold text-main">{{ $t('config.mediaPlayers') }}</span>
  </label>
  <label
    class="flex items-center gap-2 px-2 py-1.5 rounded-lg border border-black/[0.08] dark:border-white/[0.1] bg-white dark:bg-[#1c1c1e] cursor-pointer group hover:border-accent/40 transition-colors"
  >
    <input
      type="checkbox"
      v-model="fields.show_ha_scenes"
      class="rounded border-slate-300 text-accent focus:ring-accent"
    />
    <span class="text-[11px] font-bold text-main">{{ $t('config.scenes') }}</span>
  </label>
  <label
    class="flex items-center gap-2 px-2 py-1.5 rounded-lg border border-black/[0.08] dark:border-white/[0.1] bg-white dark:bg-[#1c1c1e] cursor-pointer group hover:border-accent/40 transition-colors"
  >
    <input
      type="checkbox"
      v-model="fields.show_ha_weather"
      class="rounded border-slate-300 text-accent focus:ring-accent"
    />
    <span class="text-[11px] font-bold text-main">{{ $t('config.weather') }}</span>
  </label>
</template>
<script setup lang="ts">
import type { AppConfig } from '../../config'
import { reactive } from 'vue'
import { configField } from './configField'
const config = defineModel<AppConfig>('config', { required: true })
const fields = reactive({
  show_washer: configField(config, 'show_washer'),
  show_dryer: configField(config, 'show_dryer'),
  show_dishwasher: configField(config, 'show_dishwasher'),
  show_ha_sensors: configField(config, 'show_ha_sensors'),
  show_ha_numbers: configField(config, 'show_ha_numbers'),
  show_ha_covers: configField(config, 'show_ha_covers'),
  show_ha_media: configField(config, 'show_ha_media'),
  show_ha_scenes: configField(config, 'show_ha_scenes'),
  show_ha_weather: configField(config, 'show_ha_weather'),
})
</script>
