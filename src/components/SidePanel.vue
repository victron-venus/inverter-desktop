<template>
  <div class="flex flex-col gap-1 h-full">
    <!-- EV Section -->
    <div
      v-if="
        showEv !== false &&
        (features?.ev !== false ||
          evPowerWatts > 0 ||
          evChargingKw > 0 ||
          evLoadPower > 0 ||
          (carSoc && carSoc > 0))
      "
      class="classic-card"
    >
      <div class="classic-header flex items-center gap-1.5">
        <Car :size="10" /> {{ $t('sections.ev') }}
      </div>
      <div class="p-1 flex justify-between items-center gap-2">
        <div v-if="evChargingKw > 0">
          <div class="text-xl font-bold text-solar leading-none">
            {{ evChargingKw.toFixed(1) }}kW
          </div>
          <div class="text-[10px] text-slate-500 font-bold text-center">
            {{ $t('sections.charging') }}
          </div>
        </div>
        <div class="text-center" v-if="evPowerWatts > 0">
          <div class="text-xl font-bold text-slate-500 leading-none">{{ evPower }}</div>
          <div class="text-[10px] text-slate-500 font-bold tracking-tighter">
            {{ $t('sections.vue') }}
          </div>
        </div>
        <div class="text-right">
          <div class="text-2xl font-bold text-accent leading-none">
            {{ Math.floor(carSoc || 0) }}%
          </div>
          <div class="text-[10px] text-slate-500 font-bold text-center tracking-tighter">
            {{ $t('sections.soc') }}
          </div>
        </div>
      </div>
    </div>

    <!-- Water Section -->
    <div v-if="features?.water !== false" class="classic-card">
      <div class="classic-header flex items-center gap-1.5">
        <Droplets :size="10" /> {{ $t('sections.water') }}
      </div>
      <div class="p-1 flex justify-between items-center gap-2 px-2">
        <div class="text-xl font-bold" :class="waterValve ? 'text-red-500' : 'text-green-500'">
          {{ waterLevel || 0 }} cm
        </div>
        <div class="flex gap-1">
          <button
            class="classic-btn"
            :class="{ 'classic-btn-on': pumpSwitch }"
            @click="$emit('send', 'toggle', { entity: pumpSwitchEntity })"
          >
            {{ $t('sections.pump') }}
          </button>
          <button
            class="classic-btn"
            :class="{ 'classic-btn-on': waterValve }"
            @click="$emit('send', 'toggle', { entity: waterValveEntity })"
          >
            {{ $t('sections.valve') }}
          </button>
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
      <div class="p-1 flex flex-wrap gap-0.5 overflow-y-auto max-h-[300px]">
        <button
          v-for="btn in homeButtons"
          :key="btn.id"
          class="classic-btn !flex-1 !min-w-[50px] !normal-case flex flex-col items-center gap-0.5"
          :class="{ 'classic-btn-on': buttonStates[btn.id] === 'on' }"
          @click="$emit('send', 'toggle', { entity: btn.entity })"
        >
          <component
            :is="getHomeButtonIcon(btn.entity, btn.label)"
            v-if="getHomeButtonIcon(btn.entity, btn.label)"
            :size="14"
            class="opacity-70"
          />
          <span class="text-[9px] leading-tight">{{ getHomeButtonLabel(btn.label) }}</span>
        </button>
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
        <div v-for="cover in haCovers" :key="cover.entity_id" class="flex flex-col gap-0.5">
          <div class="flex justify-between items-center px-1">
            <span
              :id="'cover-label-' + cover.entity_id"
              class="text-[10px] font-medium text-slate-500 truncate mr-2"
            >
              {{ cover.name }}
            </span>
            <span class="text-[10px] font-bold text-slate-600 dark:text-slate-400">
              {{ cover.position }}%
            </span>
          </div>
          <input
            type="range"
            :id="'cover-slider-' + cover.entity_id"
            :aria-labelledby="'cover-label-' + cover.entity_id"
            min="0"
            max="100"
            :value="cover.position"
            class="w-full h-1 accent-blue-500 cursor-pointer"
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
            <button
              class="classic-btn !px-1.5 !py-0.5 !text-[9px]"
              @click="$emit('media-control', mp.entity_id, 'play')"
            >
              ▶
            </button>
            <button
              class="classic-btn !px-1.5 !py-0.5 !text-[9px]"
              @click="$emit('media-control', mp.entity_id, 'pause')"
            >
              ⏸
            </button>
            <button
              class="classic-btn !px-1.5 !py-0.5 !text-[9px]"
              @click="$emit('media-control', mp.entity_id, 'stop')"
            >
              ⏹
            </button>
          </div>
        </div>
      </div>
    </div>

    <!-- Appliances -->
    <div
      v-if="showDishwasher !== false || showWasher !== false || showDryer !== false"
      class="flex flex-col gap-0.5"
    >
      <div
        v-if="showDishwasher !== false"
        class="classic-card px-2 py-0.5 flex justify-between items-center"
      >
        <span class="text-[10px] font-bold text-slate-500 uppercase tracking-tighter">{{
          $t('sections.dishwasher')
        }}</span>
        <div class="flex items-center gap-1.5">
          <span
            class="text-[10px] font-bold uppercase tracking-tighter"
            :class="dishwasherRunning ? 'text-green-600' : 'text-slate-400'"
            >{{ dishwasherRunning ? $t('sections.running') : 'Idle' }}</span
          >
          <span
            v-if="dishwasherDuration"
            class="text-[11px] font-bold text-slate-700 dark:text-slate-300"
            >{{ formatDuration(dishwasherDuration) }}</span
          >
        </div>
      </div>

      <div
        v-if="showWasher !== false"
        class="classic-card px-2 py-0.5 flex justify-between items-center"
      >
        <span class="text-[10px] font-bold text-slate-500 uppercase tracking-tighter">{{
          $t('sections.washer')
        }}</span>
        <div class="flex items-center gap-1.5">
          <span
            class="text-[10px] font-bold uppercase tracking-tighter"
            :class="washerRunning ? 'text-green-600' : 'text-slate-400'"
            >{{ washerRunning ? $t('sections.running') : 'Idle' }}</span
          >
          <span
            v-if="washerTime"
            class="text-[11px] font-bold text-slate-700 dark:text-slate-300"
            >{{ formatDuration(washerTime) }}</span
          >
          <div
            v-if="washerPower !== undefined"
            class="w-1.5 h-1.5 rounded-full"
            :class="washerPower ? 'bg-green-500' : 'bg-slate-200'"
          ></div>
        </div>
      </div>

      <div
        v-if="showDryer !== false"
        class="classic-card px-2 py-0.5 flex justify-between items-center"
      >
        <span class="text-[10px] font-bold text-slate-500 uppercase tracking-tighter">{{
          $t('sections.dryer')
        }}</span>
        <div class="flex items-center gap-1.5">
          <span
            class="text-[10px] font-bold uppercase tracking-tighter"
            :class="dryerRunning ? 'text-green-600' : 'text-slate-400'"
            >{{ dryerRunning ? $t('sections.running') : 'Idle' }}</span
          >
          <span v-if="dryerTime" class="text-[11px] font-bold text-slate-700 dark:text-slate-300">{{
            formatDuration(dryerTime)
          }}</span>
          <div
            v-if="dryerPower !== undefined"
            class="w-1.5 h-1.5 rounded-full"
            :class="dryerPower ? 'bg-green-500' : 'bg-slate-200'"
          ></div>
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
        <button
          v-for="scene in haScenes"
          :key="scene.entity_id"
          class="classic-btn !flex-1 !min-w-[50px] !normal-case !text-[10px]"
          @click="$emit('scene-activate', scene.entity_id)"
        >
          {{ scene.name }}
        </button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  Car,
  CloudSun,
  Droplets,
  Gauge,
  Home as HomeIcon,
  Blinds,
  Play,
  Sliders,
  Sparkles,
  Lightbulb,
  WashingMachine,
  PlugZap,
  type LucideIcon,
} from '@lucide/vue'
import { formatDuration } from '../utils'
import type {
  HaCoverDisplay,
  HaMediaPlayerDisplay,
  HaNumberDisplay,
  HaSceneDisplay,
  HaSensorDisplay,
  HaWeatherDisplay,
} from '../types/ha'

defineProps<{
  features?: Record<string, boolean>
  evCharging: string
  evPower: string
  evPowerWatts: number
  evChargingKw: number
  evLoadPower: number
  carSoc?: number
  waterLevel?: number
  waterValve?: boolean
  pumpSwitch?: boolean
  pumpSwitchEntity: string
  waterValveEntity: string
  dishwasherRunning?: boolean
  dishwasherDuration?: number
  washerRunning?: boolean
  washerTime?: number
  washerPower?: boolean
  dryerRunning?: boolean
  dryerTime?: number
  dryerPower?: boolean
  homeButtons: Array<{ id: string; label: string; entity: string }>
  buttonStates: Record<string, string>
  haSensors: HaSensorDisplay[]
  haNumbers: HaNumberDisplay[]
  haCovers: HaCoverDisplay[]
  haMediaPlayers: HaMediaPlayerDisplay[]
  haScenes: HaSceneDisplay[]
  haWeather: HaWeatherDisplay | null
  showEv?: boolean
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

defineEmits<{
  send: [action: string, payload?: Record<string, unknown>]
  'cover-position': [entityId: string, position: number]
  'media-control': [entityId: string, action: string]
  'number-set': [entityId: string, value: number]
  'scene-activate': [entityId: string]
}>()

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

/** Get display label with keywords stripped */
function getHomeButtonLabel(label: string): string {
  return label
    .replace(/\b(laundry|washer|washing|guard)\b/gi, '')
    .replace(/\s{2,}/g, ' ')
    .trim()
}
</script>
