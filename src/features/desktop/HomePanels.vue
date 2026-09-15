<template>
  <div class="flex flex-col gap-1.5">
    <output
      v-if="
        haConnected === false &&
        (hasHaHomeButtons ||
          haSensors.length ||
          haNumbers.length ||
          haCovers.length ||
          haMediaPlayers.length ||
          haWeather)
      "
      class="text-[10px] text-consumption px-1"
    >
      {{ $t('status.haStale') }}
    </output>

    <!-- HA Weather -->
    <div v-if="haWeather && appConfig?.show_ha_weather !== false" class="classic-card">
      <div class="classic-header flex items-center gap-1.5">
        <CloudSun :size="10" /> {{ haWeather.name }}
      </div>
      <div class="p-1">
        <div class="flex items-center justify-between">
          <div class="flex items-center gap-1">
            <span class="text-lg font-bold text-main tabular tracking-tight">
              {{ haWeather.temperature }}{{ haWeather.unit }}
            </span>
            <span class="text-[10px] text-muted capitalize">{{ haWeather.state }}</span>
          </div>
        </div>
        <!-- Forecast -->
        <div v-if="haWeather.forecast.length > 0" class="mt-1 flex gap-1 overflow-x-auto">
          <div
            v-for="(day, idx) in haWeather.forecast.slice(0, 5)"
            :key="idx"
            class="classic-inset flex flex-col items-center min-w-[40px] !px-1 !py-0.5"
          >
            <span class="text-[8px] text-muted">{{
              (day.datetime as string)?.slice(5, 10) || ''
            }}</span>
            <span class="text-[10px] font-bold">{{ day.temperature }}{{ haWeather.unit }}</span>
            <span class="text-[8px] text-muted capitalize truncate max-w-[36px]">{{
              day.condition as string
            }}</span>
          </div>
        </div>
      </div>
    </div>

    <!-- HA Sensors (collapsed by default) -->
    <div v-if="haSensors.length > 0 && appConfig?.show_ha_sensors !== false" class="classic-card">
      <button
        type="button"
        class="w-full text-left classic-header flex items-center gap-1.5 cursor-pointer hover:opacity-80"
        @click="sensorsExpanded = !sensorsExpanded"
      >
        <Gauge :size="10" /> {{ $t('sections.sensors') }} ({{ haSensors.length }})
        <span class="ml-auto text-[10px]">{{ sensorsExpanded ? '▾' : '▸' }}</span>
      </button>
      <div v-if="sensorsExpanded" class="p-1 flex flex-col gap-0.5">
        <div
          v-for="sensor in haSensors"
          :key="sensor.entity_id"
          class="row-hover flex justify-between items-center px-1 py-0.5 rounded"
        >
          <span class="text-[10px] font-medium text-muted truncate mr-2">
            {{ sensor.name }}
          </span>
          <span class="text-[11px] font-semibold text-main whitespace-nowrap tabular">
            {{ sensor.state }}{{ sensor.unit }}
          </span>
        </div>
      </div>
    </div>

    <!-- HA Numbers (collapsed by default) -->
    <div v-if="haNumbers.length > 0 && appConfig?.show_ha_numbers !== false" class="classic-card">
      <button
        type="button"
        class="w-full text-left classic-header flex items-center gap-1.5 cursor-pointer hover:opacity-80"
        @click="numbersExpanded = !numbersExpanded"
      >
        <Sliders :size="10" /> {{ $t('sections.numbers') }} ({{ haNumbers.length }})
        <span class="ml-auto text-[10px]">{{ numbersExpanded ? '▾' : '▸' }}</span>
      </button>
      <div v-if="numbersExpanded" class="p-1 flex flex-col gap-1">
        <div v-for="num in haNumbers" :key="num.entity_id" class="flex flex-col gap-0.5">
          <div class="flex justify-between items-center px-1">
            <span
              :id="'num-label-' + num.entity_id"
              class="text-[10px] font-medium text-muted truncate mr-2"
            >
              {{ num.name }}
            </span>
            <span class="text-[10px] font-semibold text-muted tabular">
              {{ num.value }}{{ num.unit }}
            </span>
          </div>
          <input
            type="range"
            :id="'num-slider-' + num.entity_id"
            :aria-labelledby="'num-label-' + num.entity_id"
            :min="num.min"
            :max="num.max"
            :step="num.step"
            :value="num.value"
            :disabled="haConnected === false"
            class="w-full h-1 accent-accent cursor-pointer"
            @change="
              $emit('number-set', num.entity_id, Number(($event.target as HTMLInputElement).value))
            "
          />
        </div>
      </div>
    </div>

    <!-- HA Covers (collapsed by default) -->
    <div v-if="haCovers.length > 0 && appConfig?.show_ha_covers !== false" class="classic-card">
      <button
        type="button"
        class="w-full text-left classic-header flex items-center gap-1.5 cursor-pointer hover:opacity-80"
        @click="coversExpanded = !coversExpanded"
      >
        <Blinds :size="10" /> {{ $t('sections.covers') }} ({{ haCovers.length }})
        <span class="ml-auto text-[10px]">{{ coversExpanded ? '▾' : '▸' }}</span>
      </button>
      <div v-if="coversExpanded" class="p-1 flex flex-col gap-1">
        <div
          v-for="cover in haCovers"
          :key="cover.entity_id"
          class="flex flex-col gap-0.5"
          :class="{ 'ha-entity-unavailable': isCoverUnavailable(cover) }"
        >
          <div class="flex justify-between items-center px-1">
            <span
              :id="'cover-label-' + cover.entity_id"
              class="text-[10px] font-medium text-muted truncate mr-2"
            >
              {{ cover.name }}
            </span>
            <span class="text-[10px] font-semibold text-muted tabular">
              <template v-if="isCoverUnavailable(cover)">unavailable</template>
              <template v-else>{{ coverStateLabel(cover) }} · {{ cover.position }}%</template>
            </span>
          </div>
          <input
            type="range"
            :id="'cover-slider-' + cover.entity_id"
            :aria-labelledby="'cover-label-' + cover.entity_id"
            min="0"
            max="100"
            :value="cover.position"
            :disabled="haConnected === false || isCoverUnavailable(cover)"
            class="w-full h-1 accent-accent cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed"
            @change="
              $emit(
                'cover-position',
                cover.entity_id,
                Number(($event.target as HTMLInputElement).value)
              )
            "
          />
        </div>
      </div>
    </div>

    <!-- HA Media Players (collapsed by default) -->
    <div
      v-if="haMediaPlayers.length > 0 && appConfig?.show_ha_media !== false"
      class="classic-card"
    >
      <button
        type="button"
        class="w-full text-left classic-header flex items-center gap-1.5 cursor-pointer hover:opacity-80"
        @click="mediaExpanded = !mediaExpanded"
      >
        <Play :size="10" /> {{ $t('sections.media') }} ({{ haMediaPlayers.length }})
        <span class="ml-auto text-[10px]">{{ mediaExpanded ? '▾' : '▸' }}</span>
      </button>
      <div v-if="mediaExpanded" class="p-1 flex flex-col gap-0.5">
        <div
          v-for="mp in haMediaPlayers"
          :key="mp.entity_id"
          class="flex items-center justify-between px-1 py-0.5"
        >
          <div class="flex flex-col min-w-0 mr-2">
            <span class="text-[10px] font-medium text-muted truncate">{{ mp.name }}</span>
            <span class="text-[9px] text-muted truncate">{{ mp.state }}</span>
          </div>
          <div class="flex gap-0.5 shrink-0">
            <UiButton
              size="sm"
              class="!px-1.5"
              :disabled="haConnected === false"
              @click="$emit('media-control', mp.entity_id, 'play')"
            >
              ▶
            </UiButton>
            <UiButton
              size="sm"
              class="!px-1.5"
              :disabled="haConnected === false"
              @click="$emit('media-control', mp.entity_id, 'pause')"
            >
              ⏸
            </UiButton>
            <UiButton
              size="sm"
              class="!px-1.5"
              :disabled="haConnected === false"
              @click="$emit('media-control', mp.entity_id, 'stop')"
            >
              ⏹
            </UiButton>
          </div>
        </div>
      </div>
    </div>

    <!-- Appliances -->
    <div
      v-if="
        (showDishwasher !== false && dishwasherActive) ||
        (showWasher !== false && washerActive) ||
        (showDryer !== false && dryerActive)
      "
      class="flex flex-col gap-0.5"
    >
      <div
        v-if="showDishwasher !== false && dishwasherActive"
        class="classic-card px-2 py-1 flex justify-between items-center"
      >
        <span class="text-[10px] font-semibold text-muted tracking-tight">{{
          $t('sections.dishwasher')
        }}</span>
        <div class="flex items-center gap-1.5">
          <span class="text-[10px] font-semibold text-battery tracking-tight">{{
            $t('sections.running')
          }}</span>
          <span v-if="dishwasherRuntime" class="text-[11px] font-semibold text-main tabular">{{
            dishwasherRuntime
          }}</span>
        </div>
      </div>

      <div
        v-if="showWasher !== false && washerActive"
        class="classic-card px-2 py-1 flex items-center gap-1.5"
      >
        <span class="text-[10px] font-semibold text-muted tracking-tight shrink-0">{{
          $t('sections.washer')
        }}</span>
        <div class="ml-auto flex items-center gap-1.5 shrink-0">
          <div v-if="washerStartEntity || washerPauseEntity" class="flex gap-1">
            <UiButton
              v-if="washerStartEntity"
              size="sm"
              variant="primary"
              :disabled="haConnected === false"
              @click="$emit('send', 'press', { entity: washerStartEntity })"
            >
              {{ $t('sections.start') }}
            </UiButton>
            <UiButton
              v-if="washerPauseEntity"
              size="sm"
              :disabled="haConnected === false"
              @click="$emit('send', 'press', { entity: washerPauseEntity })"
            >
              {{ $t('sections.pause') }}
            </UiButton>
          </div>
          <span class="text-[10px] font-semibold text-battery tracking-tight">{{
            $t('sections.running')
          }}</span>
          <span v-if="washerRemainingTime" class="text-[11px] font-semibold text-main tabular">{{
            washerRemainingTime
          }}</span>
        </div>
      </div>

      <div
        v-if="showDryer !== false && dryerActive"
        class="classic-card px-2 py-1 flex items-center gap-1.5"
      >
        <span class="text-[10px] font-semibold text-muted tracking-tight shrink-0">{{
          $t('sections.dryer')
        }}</span>
        <div class="ml-auto flex items-center gap-1.5 shrink-0">
          <div v-if="dryerStartEntity || dryerPauseEntity" class="flex gap-1">
            <UiButton
              v-if="dryerStartEntity"
              size="sm"
              variant="primary"
              :disabled="haConnected === false"
              @click="$emit('send', 'press', { entity: dryerStartEntity })"
            >
              {{ $t('sections.start') }}
            </UiButton>
            <UiButton
              v-if="dryerPauseEntity"
              size="sm"
              :disabled="haConnected === false"
              @click="$emit('send', 'press', { entity: dryerPauseEntity })"
            >
              {{ $t('sections.pause') }}
            </UiButton>
          </div>
          <span class="text-[10px] font-semibold text-battery tracking-tight">{{
            $t('sections.running')
          }}</span>
          <span v-if="dryerRemainingTime" class="text-[11px] font-semibold text-main tabular">{{
            dryerRemainingTime
          }}</span>
        </div>
      </div>
    </div>

    <!-- HA Scenes (collapsed by default) -->
    <div v-if="haScenes.length > 0 && appConfig?.show_ha_scenes !== false" class="classic-card">
      <button
        type="button"
        class="w-full text-left classic-header flex items-center gap-1.5 cursor-pointer hover:opacity-80"
        @click="scenesExpanded = !scenesExpanded"
      >
        <Sparkles :size="10" /> {{ $t('sections.scenes') }} ({{ haScenes.length }})
        <span class="ml-auto text-[10px]">{{ scenesExpanded ? '▾' : '▸' }}</span>
      </button>
      <div v-if="scenesExpanded" class="p-1 flex flex-wrap gap-0.5">
        <UiButton
          v-for="scene in haScenes"
          :key="scene.entity_id"
          class="!flex-1 !min-w-[50px] !text-[10px]"
          :disabled="haConnected === false"
          @click="$emit('scene-activate', scene.entity_id)"
        >
          {{ scene.name }}
        </UiButton>
      </div>
    </div>
  </div>
</template>
<script setup lang="ts">
import { Blinds, CloudSun, Gauge, Play, Sliders, Sparkles } from '@lucide/vue'
import { computed, ref } from 'vue'
import UiButton from '../../components/UiButton.vue'
import { useI18n } from 'vue-i18n'
import { isHaUnavailableState } from '../../utils'
import { isInverterControlFlag } from '../../inverterControl'
import type {
  HaCoverDisplay,
  HaMediaPlayerDisplay,
  HaNumberDisplay,
  HaSceneDisplay,
  HaSensorDisplay,
  HaWeatherDisplay,
} from '../../types/ha'
const props = defineProps<{
  haConnected?: boolean
  homeButtons: Array<{ entity: string }>
  washerActive?: boolean
  washerRemainingTime?: string | null
  dryerActive?: boolean
  dryerRemainingTime?: string | null
  washerStartEntity?: string
  washerPauseEntity?: string
  dryerStartEntity?: string
  dryerPauseEntity?: string
  dishwasherActive?: boolean
  dishwasherRuntime?: string | null
  haSensors: HaSensorDisplay[]
  haNumbers: HaNumberDisplay[]
  haCovers: HaCoverDisplay[]
  haMediaPlayers: HaMediaPlayerDisplay[]
  haScenes: HaSceneDisplay[]
  haWeather: HaWeatherDisplay | null
  showWasher?: boolean
  showDryer?: boolean
  showDishwasher?: boolean
  appConfig?: {
    show_ha_sensors?: boolean
    show_ha_numbers?: boolean
    show_ha_covers?: boolean
    show_ha_media?: boolean
    show_ha_scenes?: boolean
    show_ha_weather?: boolean
  } | null
}>()
defineEmits<{
  send: [action: string, payload?: Record<string, unknown>]
  'cover-position': [entityId: string, position: number]
  'media-control': [entityId: string, action: string]
  'number-set': [entityId: string, value: number]
  'scene-activate': [entityId: string]
}>()
const hasHaHomeButtons = computed(() =>
  props.homeButtons.some((entry) => !isInverterControlFlag(entry.entity))
)
const { t: $t } = useI18n()
const sensorsExpanded = ref(false)
const numbersExpanded = ref(false)
const coversExpanded = ref(false)
const mediaExpanded = ref(false)
const scenesExpanded = ref(false)
function isCoverUnavailable(cover: HaCoverDisplay): boolean {
  return isHaUnavailableState(cover.state)
}
function coverStateLabel(cover: HaCoverDisplay): string {
  const state = (cover.state || '').trim().toLowerCase()
  if (state === 'open' || state === 'opening') return 'open'
  if (state === 'closed' || state === 'closing') return 'closed'
  return state || (cover.position > 0 ? 'open' : 'closed')
}
</script>
