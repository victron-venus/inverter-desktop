<template>
  <div class="flex flex-col gap-1 h-full">
    <!-- EV Section -->
    <div v-if="showEv !== false && evSectionVisible" class="classic-card">
      <div class="classic-header flex items-center gap-1.5">
        <Car :size="10" /> {{ $t('sections.ev') }}
      </div>
      <div class="p-1 flex justify-between items-center gap-2 px-2">
        <div v-if="carChargingPower != null">
          <div class="text-xl font-bold text-solar leading-none">
            {{ (carChargingPower / 1000).toFixed(1) }}kW
          </div>
          <div class="text-[10px] text-slate-500 font-bold text-center">
            {{ $t('sections.charging') }}
          </div>
        </div>
        <div class="text-center" v-if="evChargingPower !== null">
          <div class="text-xl font-bold text-slate-500 leading-none">
            {{ (evChargingPower / 1000).toFixed(1) }}kW
          </div>
          <div class="text-[10px] text-slate-500 font-bold tracking-tighter">
            {{ $t('sections.evcharger') }}
          </div>
        </div>
        <div class="text-right" v-if="carSoc != null && carSoc > 0">
          <div class="text-xl font-bold text-accent leading-none">{{ Math.floor(carSoc) }}%</div>
          <div class="text-[10px] text-slate-500 font-bold text-center tracking-tighter">
            {{ $t('sections.soc') }}
          </div>
        </div>
      </div>
    </div>

    <!-- Water Section (dbus-pump via Cerbo MQTT; control lives in dbus-pump) -->
    <div v-if="waterVisible" class="classic-card">
      <div class="classic-header flex items-center gap-1.5">
        <Droplets :size="10" /> {{ $t('sections.water') }}
      </div>
      <div class="p-1 flex justify-between items-center gap-2 px-2">
        <div
          v-if="waterLevel != null"
          class="text-xl font-bold"
          :class="waterValve === true ? 'text-red-500' : 'text-green-500'"
        >
          {{ Math.round(waterLevel) }} %
        </div>
        <div class="flex gap-1 items-center">
          <UiButton
            v-if="pumpSwitch != null"
            size="sm"
            toggle
            :active="pumpSwitch === true"
            @click="$emit('send', 'water_mode', { which: 'pump', mode: pumpSwitch ? 2 : 1 })"
          >
            {{ $t('sections.pump') }}
          </UiButton>
          <UiButton
            v-if="waterValve != null"
            size="sm"
            toggle
            :active="waterValve === true"
            @click="onValveClick"
          >
            {{ $t('sections.valve') }}
          </UiButton>
          <!-- Reset chip shown only while dbus-pump /Mode is a manual override -->
          <UiButton
            v-if="waterPumpMode === 1 || waterPumpMode === 2"
            size="sm"
            variant="danger"
            @click="$emit('send', 'water_mode', { which: 'pump', mode: 0 })"
          >
            {{ $t('sections.auto') }}
          </UiButton>
          <UiButton
            v-if="waterValveMode === 1 || waterValveMode === 2"
            size="sm"
            variant="danger"
            @click="$emit('send', 'water_mode', { which: 'valve', mode: 0 })"
          >
            {{ $t('sections.auto') }}
          </UiButton>
        </div>
      </div>
    </div>

    <!-- Home Controls -->
    <div
      v-if="features?.ha !== false && showHomeSection !== false && homeButtons.length > 0"
      class="classic-card flex-1 min-h-0"
    >
      <div class="classic-header flex items-center gap-1.5">
        <HomeIcon :size="10" /> {{ $t('sections.home') }}
      </div>
      <div class="home-btn-grid p-1 overflow-y-auto max-h-[300px]">
        <UiButton
          v-for="btn in homeButtons"
          :key="btn.id"
          variant="tile"
          class="home-btn-tile"
          toggle
          :active="buttonStates[btn.id] === 'on'"
          :unavailable="isBtnUnavailable(buttonStates[btn.id])"
          @click="$emit('send', 'toggle', { entity: btn.entity })"
        >
          <component
            :is="getHomeButtonIcon(btn.entity, btn.label)"
            v-if="getHomeButtonIcon(btn.entity, btn.label)"
            :size="12"
            class="opacity-70 shrink-0"
          />
          <span class="home-tile-label">{{ getHomeButtonLabel(btn.label) }}</span>
        </UiButton>
      </div>
    </div>

    <!-- HA Weather -->
    <div v-if="haWeather && appConfig?.show_ha_weather !== false" class="classic-card">
      <div class="classic-header flex items-center gap-1.5">
        <CloudSun :size="10" /> {{ haWeather.name }}
      </div>
      <div class="p-1">
        <div class="flex items-center justify-between">
          <div class="flex items-center gap-1">
            <span class="text-lg font-bold text-slate-700 dark:text-slate-300">
              {{ haWeather.temperature }}{{ haWeather.unit }}
            </span>
            <span class="text-[10px] text-slate-500 capitalize">{{ haWeather.state }}</span>
          </div>
        </div>
        <!-- Forecast -->
        <div v-if="haWeather.forecast.length > 0" class="mt-1 flex gap-1 overflow-x-auto">
          <div
            v-for="(day, idx) in haWeather.forecast.slice(0, 5)"
            :key="idx"
            class="flex flex-col items-center min-w-[40px] px-1 py-0.5 rounded bg-slate-50 dark:bg-slate-800/50"
          >
            <span class="text-[8px] text-slate-400">{{
              (day.datetime as string)?.slice(5, 10) || ''
            }}</span>
            <span class="text-[10px] font-bold">{{ day.temperature }}{{ haWeather.unit }}</span>
            <span class="text-[8px] text-slate-500 capitalize truncate max-w-[36px]">{{
              day.condition as string
            }}</span>
          </div>
        </div>
      </div>
    </div>

    <!-- HA Sensors (collapsed by default) -->
    <div v-if="haSensors.length > 0 && appConfig?.show_ha_sensors !== false" class="classic-card">
      <div
        class="classic-header flex items-center gap-1.5 cursor-pointer hover:opacity-80"
        @click="sensorsExpanded = !sensorsExpanded"
      >
        <Gauge :size="10" /> {{ $t('sections.sensors') }} ({{ haSensors.length }})
        <span class="ml-auto text-[10px]">{{ sensorsExpanded ? '▼' : '▶' }}</span>
      </div>
      <div v-if="sensorsExpanded" class="p-1 flex flex-col gap-0.5">
        <div
          v-for="sensor in haSensors"
          :key="sensor.entity_id"
          class="flex justify-between items-center px-1 py-0.5 rounded hover:bg-slate-50 dark:hover:bg-slate-800/50"
        >
          <span class="text-[10px] font-medium text-slate-500 truncate mr-2">
            {{ sensor.name }}
          </span>
          <span class="text-[11px] font-bold text-slate-700 dark:text-slate-300 whitespace-nowrap">
            {{ sensor.state }}{{ sensor.unit }}
          </span>
        </div>
      </div>
    </div>

    <!-- HA Numbers (collapsed by default) -->
    <div v-if="haNumbers.length > 0 && appConfig?.show_ha_numbers !== false" class="classic-card">
      <div
        class="classic-header flex items-center gap-1.5 cursor-pointer hover:opacity-80"
        @click="numbersExpanded = !numbersExpanded"
      >
        <Sliders :size="10" /> {{ $t('sections.numbers') }} ({{ haNumbers.length }})
        <span class="ml-auto text-[10px]">{{ numbersExpanded ? '▼' : '▶' }}</span>
      </div>
      <div v-if="numbersExpanded" class="p-1 flex flex-col gap-1">
        <div v-for="num in haNumbers" :key="num.entity_id" class="flex flex-col gap-0.5">
          <div class="flex justify-between items-center px-1">
            <span
              :id="'num-label-' + num.entity_id"
              class="text-[10px] font-medium text-slate-500 truncate mr-2"
            >
              {{ num.name }}
            </span>
            <span class="text-[10px] font-bold text-slate-600 dark:text-slate-400">
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
            class="w-full h-1 accent-blue-500 cursor-pointer"
            @change="
              $emit('number-set', num.entity_id, Number(($event.target as HTMLInputElement).value))
            "
          />
        </div>
      </div>
    </div>

    <!-- HA Covers (collapsed by default) -->
    <div v-if="haCovers.length > 0 && appConfig?.show_ha_covers !== false" class="classic-card">
      <div
        class="classic-header flex items-center gap-1.5 cursor-pointer hover:opacity-80"
        @click="coversExpanded = !coversExpanded"
      >
        <Blinds :size="10" /> {{ $t('sections.covers') }} ({{ haCovers.length }})
        <span class="ml-auto text-[10px]">{{ coversExpanded ? '▼' : '▶' }}</span>
      </div>
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
              class="text-[10px] font-medium text-slate-500 truncate mr-2"
            >
              {{ cover.name }}
            </span>
            <span class="text-[10px] font-bold text-slate-600 dark:text-slate-400">
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
            :disabled="isCoverUnavailable(cover)"
            class="w-full h-1 accent-blue-500 cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed"
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
      <div
        class="classic-header flex items-center gap-1.5 cursor-pointer hover:opacity-80"
        @click="mediaExpanded = !mediaExpanded"
      >
        <Play :size="10" /> {{ $t('sections.media') }} ({{ haMediaPlayers.length }})
        <span class="ml-auto text-[10px]">{{ mediaExpanded ? '▼' : '▶' }}</span>
      </div>
      <div v-if="mediaExpanded" class="p-1 flex flex-col gap-0.5">
        <div
          v-for="mp in haMediaPlayers"
          :key="mp.entity_id"
          class="flex items-center justify-between px-1 py-0.5"
        >
          <div class="flex flex-col min-w-0 mr-2">
            <span class="text-[10px] font-medium text-slate-500 truncate">{{ mp.name }}</span>
            <span class="text-[9px] text-slate-400 truncate">{{ mp.state }}</span>
          </div>
          <div class="flex gap-0.5 shrink-0">
            <UiButton
              size="sm"
              class="!px-1.5"
              @click="$emit('media-control', mp.entity_id, 'play')"
            >
              ▶
            </UiButton>
            <UiButton
              size="sm"
              class="!px-1.5"
              @click="$emit('media-control', mp.entity_id, 'pause')"
            >
              ⏸
            </UiButton>
            <UiButton
              size="sm"
              class="!px-1.5"
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
        class="classic-card px-2 py-0.5 flex justify-between items-center"
      >
        <span class="text-[10px] font-bold text-slate-500 uppercase tracking-tighter">{{
          $t('sections.dishwasher')
        }}</span>
        <div class="flex items-center gap-1.5">
          <span class="text-[10px] font-bold text-green-600 uppercase tracking-tighter">{{
            $t('sections.running')
          }}</span>
          <span
            v-if="dishwasherRemainingTime"
            class="text-[11px] font-bold text-slate-700 dark:text-slate-300"
            >{{ dishwasherRemainingTime }}</span
          >
        </div>
      </div>

      <div
        v-if="showWasher !== false && washerActive"
        class="classic-card px-2 py-0.5 flex flex-col gap-0.5"
      >
        <div class="flex justify-between items-center">
          <span class="text-[10px] font-bold text-slate-500 uppercase tracking-tighter">{{
            $t('sections.washer')
          }}</span>
          <div class="flex items-center gap-1.5">
            <span class="text-[10px] font-bold text-green-600 uppercase tracking-tighter">{{
              $t('sections.running')
            }}</span>
            <span
              v-if="washerRemainingTime"
              class="text-[11px] font-bold text-slate-700 dark:text-slate-300"
              >{{ washerRemainingTime }}</span
            >
          </div>
        </div>
        <div v-if="washerStartEntity || washerPauseEntity" class="flex gap-1 justify-end">
          <UiButton
            v-if="washerStartEntity"
            size="sm"
            variant="primary"
            @click="$emit('send', 'press', { entity: washerStartEntity })"
          >
            {{ $t('sections.start') }}
          </UiButton>
          <UiButton
            v-if="washerPauseEntity"
            size="sm"
            @click="$emit('send', 'press', { entity: washerPauseEntity })"
          >
            {{ $t('sections.pause') }}
          </UiButton>
        </div>
      </div>

      <div
        v-if="showDryer !== false && dryerActive"
        class="classic-card px-2 py-0.5 flex flex-col gap-0.5"
      >
        <div class="flex justify-between items-center">
          <span class="text-[10px] font-bold text-slate-500 uppercase tracking-tighter">{{
            $t('sections.dryer')
          }}</span>
          <div class="flex items-center gap-1.5">
            <span class="text-[10px] font-bold text-green-600 uppercase tracking-tighter">{{
              $t('sections.running')
            }}</span>
            <span
              v-if="dryerRemainingTime"
              class="text-[11px] font-bold text-slate-700 dark:text-slate-300"
              >{{ dryerRemainingTime }}</span
            >
          </div>
        </div>
        <div v-if="dryerStartEntity || dryerPauseEntity" class="flex gap-1 justify-end">
          <UiButton
            v-if="dryerStartEntity"
            size="sm"
            variant="primary"
            @click="$emit('send', 'press', { entity: dryerStartEntity })"
          >
            {{ $t('sections.start') }}
          </UiButton>
          <UiButton
            v-if="dryerPauseEntity"
            size="sm"
            @click="$emit('send', 'press', { entity: dryerPauseEntity })"
          >
            {{ $t('sections.pause') }}
          </UiButton>
        </div>
      </div>
    </div>

    <!-- HA Scenes (collapsed by default) -->
    <div v-if="haScenes.length > 0 && appConfig?.show_ha_scenes !== false" class="classic-card">
      <div
        class="classic-header flex items-center gap-1.5 cursor-pointer hover:opacity-80"
        @click="scenesExpanded = !scenesExpanded"
      >
        <Sparkles :size="10" /> {{ $t('sections.scenes') }} ({{ haScenes.length }})
        <span class="ml-auto text-[10px]">{{ scenesExpanded ? '▼' : '▶' }}</span>
      </div>
      <div v-if="scenesExpanded" class="p-1 flex flex-wrap gap-0.5">
        <UiButton
          v-for="scene in haScenes"
          :key="scene.entity_id"
          class="!flex-1 !min-w-[50px] !text-[10px]"
          @click="$emit('scene-activate', scene.entity_id)"
        >
          {{ scene.name }}
        </UiButton>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import {
  Blinds,
  Car,
  CloudSun,
  Droplets,
  Gauge,
  Home as HomeIcon,
  Lightbulb,
  type LucideIcon,
  Play,
  PlugZap,
  Sliders,
  Sparkles,
  WashingMachine,
} from '@lucide/vue'
import { ref } from 'vue'
import UiButton from './UiButton.vue'
import { useI18n } from 'vue-i18n'
import { isHaUnavailableState } from '../utils'
import type {
  HaCoverDisplay,
  HaMediaPlayerDisplay,
  HaNumberDisplay,
  HaSceneDisplay,
  HaSensorDisplay,
  HaWeatherDisplay,
} from '../types/ha'

const props = defineProps<{
  features?: Record<string, boolean>
  showEv?: boolean
  evSectionVisible?: boolean
  carSoc?: number | null
  carChargingPower?: number | null
  evChargingPower?: number | null
  waterVisible?: boolean
  waterValve?: boolean | null
  pumpSwitch?: boolean | null
  waterPumpMode?: number | null
  waterValveMode?: number | null
  waterLevel?: number | null
  washerActive?: boolean
  washerRemainingTime?: string | null
  dryerActive?: boolean
  dryerRemainingTime?: string | null
  washerStartEntity?: string
  washerPauseEntity?: string
  dryerStartEntity?: string
  dryerPauseEntity?: string
  dishwasherActive?: boolean
  dishwasherRemainingTime?: string | null
  homeButtons: Array<{ id: string; label: string; entity: string }>
  buttonStates: Record<string, string>
  haSensors: HaSensorDisplay[]
  haNumbers: HaNumberDisplay[]
  haCovers: HaCoverDisplay[]
  haMediaPlayers: HaMediaPlayerDisplay[]
  haScenes: HaSceneDisplay[]
  haWeather: HaWeatherDisplay | null
  showWasher?: boolean
  showDryer?: boolean
  showDishwasher?: boolean
  showHomeSection?: boolean
  appConfig?: {
    show_ha_sensors?: boolean
    show_ha_numbers?: boolean
    show_ha_covers?: boolean
    show_ha_media?: boolean
    show_ha_scenes?: boolean
    show_ha_weather?: boolean
  } | null
}>()

const emit = defineEmits<{
  send: [action: string, payload?: Record<string, unknown>]
  'cover-position': [entityId: string, position: number]
  'media-control': [entityId: string, action: string]
  'number-set': [entityId: string, value: number]
  'scene-activate': [entityId: string]
}>()

// Opening the city valve floods the house system from the mains - confirm.
function onValveClick() {
  if (props.waterValve === false && !window.confirm('Open city water valve?')) return
  emit('send', 'water_mode', { which: 'valve', mode: props.waterValve ? 2 : 1 })
}

const { t: $t } = useI18n()

// HA sections collapsed by default
const sensorsExpanded = ref(false)
const numbersExpanded = ref(false)
const coversExpanded = ref(false)
const mediaExpanded = ref(false)
const scenesExpanded = ref(false)

/** Resolve icon for a home button based on entity domain and label */
function getHomeButtonIcon(entity: string, label: string): LucideIcon | null {
  const domain = entity.split('.')[0]
  const lowerLabel = label.toLowerCase()

  if (domain === 'light') return Lightbulb
  if (
    lowerLabel.includes('laundry') ||
    lowerLabel.includes('washer') ||
    lowerLabel.includes('washing')
  ) {
    return WashingMachine
  }
  if (lowerLabel.includes('guard')) return PlugZap
  // plug/socket → no icon, just text
  return null
}

/** Get display label with keywords stripped; break at spaces for compact tiles. */
function getHomeButtonLabel(label: string): string {
  return label
    .replace(/\b(laundry|washer|washing|guard)\b/gi, '')
    .replace(/\s{2,}/g, ' ')
    .trim()
    .split(' ')
    .filter(Boolean)
    .join('\n')
}

function isBtnUnavailable(state: string | undefined): boolean {
  return isHaUnavailableState(state)
}

function isCoverUnavailable(cover: HaCoverDisplay): boolean {
  return isHaUnavailableState(cover.state)
}

function coverStateLabel(cover: HaCoverDisplay): string {
  const s = (cover.state || '').trim().toLowerCase()
  if (s === 'open' || s === 'opening') return 'open'
  if (s === 'closed' || s === 'closing') return 'closed'
  if (s) return s
  return cover.position > 0 ? 'open' : 'closed'
}
</script>
