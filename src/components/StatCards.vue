<template>
  <div class="stat-cards grid grid-cols-2 md:grid-cols-5">
    <!-- Grid -->
    <div
      class="classic-card metric-card px-2 py-2 flex flex-col items-center justify-center min-h-[76px]"
    >
      <div class="classic-stat-label flex flex-wrap items-center justify-center gap-1">
        Grid
        <span
          v-if="gridBackup?.service"
          data-testid="grid-backup"
          class="normal-case tracking-normal font-normal"
          :class="backupActive ? 'text-accent' : 'opacity-70'"
          :title="backupHint"
        >
          · {{ backupPower }}
        </span>
      </div>
      <div class="classic-stat-value text-[1.75rem] font-bold text-grid">
        {{ formatGridPower(gtH) }}
      </div>
      <div class="classic-stat-meta mt-0.5">
        {{ formatGridPower(g1H) }} <span class="opacity-30 mx-0.5">·</span>
        {{ formatGridPower(g2H) }}
      </div>
    </div>

    <!-- Consumption -->
    <div
      class="classic-card metric-card px-2 py-2 flex flex-col items-center justify-center min-h-[76px]"
    >
      <div class="classic-stat-label">Consumption</div>
      <div class="classic-stat-value text-[1.75rem] font-bold text-consumption">
        {{ formatPower(ttH) }}
      </div>
      <div class="classic-stat-meta mt-0.5">
        {{ formatPower(t1H) }} <span class="opacity-30 mx-0.5">·</span> {{ formatPower(t2H) }}
      </div>
    </div>

    <!-- Solar -->
    <div
      class="classic-card metric-card px-2 py-2 flex flex-col items-center justify-center min-h-[76px]"
    >
      <div class="classic-stat-label">Solar</div>
      <div class="classic-stat-value text-[1.75rem] font-bold text-solar">
        {{ formatPower(solarTotalH) }}
      </div>
      <div class="classic-stat-meta mt-0.5">
        {{ formatPower(mpptTotalH) }} <span class="opacity-30 mx-0.5">·</span>
        {{ formatPower(pvInvertersTotalH) }}
      </div>
    </div>

    <!-- Battery -->
    <div
      class="classic-card metric-card px-2 py-2 flex flex-col items-center justify-center min-h-[76px]"
    >
      <div class="classic-stat-label">Battery</div>
      <div class="classic-stat-value text-[1.75rem] font-bold text-battery">
        {{ Math.floor(batterySocH ?? 0) }}%
      </div>
      <div class="classic-stat-meta mt-0.5 truncate w-full text-center">
        {{ formatPower(batteryPowerH) }} <span class="opacity-30 mx-0.5">·</span>
        {{ (batteryVoltageH ?? 0).toFixed(2) }}V <span class="opacity-30 mx-0.5">·</span>
        {{ (batteryCurrentH ?? 0).toFixed(1) }}A
      </div>
    </div>

    <!-- Setpoint -->
    <div
      class="classic-card metric-card metric-card-setpoint px-2 py-2 flex flex-col items-center justify-center min-h-[76px]"
    >
      <div class="classic-stat-label flex items-center justify-center gap-1">
        Setpoint
        <SetpointOverride :current-setpoint="setpointH" />
      </div>
      <div class="classic-stat-value text-[1.75rem] font-bold text-accent">
        {{ formatPower(setpointH) }}
      </div>
      <div class="classic-stat-meta mt-0.5 truncate w-full text-center">
        {{ formatInverterState(inverterState) }}
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { formatInverterState, formatPower, holdNumber } from '../utils'
import type { GridBackupStatus } from '../composables/useInverterState'
import SetpointOverride from './SetpointOverride.vue'

const props = withDefaults(
  defineProps<{
    gt?: number
    g1?: number
    g2?: number
    gridL1Available?: boolean
    gridL2Available?: boolean
    gridBackup?: GridBackupStatus | null
    gridUsingBackup?: boolean
    gridBackupObservedAt?: number
    tt?: number
    t1?: number
    t2?: number
    solarTotal?: number
    mpptTotal: number
    pvInvertersTotal: number
    batterySoc?: number
    batteryPower?: number
    batteryVoltage?: number
    batteryCurrent?: number
    setpoint?: number
    inverterState?: string
  }>(),
  {
    gridL1Available: undefined,
    gridL2Available: undefined,
  }
)

const now = ref(Date.now() / 1000)
let freshnessTimer: ReturnType<typeof setInterval> | undefined
onMounted(() => {
  freshnessTimer = setInterval(() => {
    now.value = Date.now() / 1000
  }, 1000)
})
onUnmounted(() => clearInterval(freshnessTimer))
const backupLive = computed(() => {
  const observed = props.gridBackupObservedAt
  return (
    typeof observed === 'number' &&
    Number.isFinite(observed) &&
    now.value - observed <= 30 &&
    now.value - observed >= -5
  )
})
const backupActive = computed(
  () => props.gridUsingBackup && backupLive.value && props.gridBackup?.available
)
const backupPower = computed(() => {
  const backup = props.gridBackup
  return backupLive.value &&
    backup?.available &&
    typeof backup.power === 'number' &&
    Number.isFinite(backup.power)
    ? formatPower(backup.power)
    : '—'
})
const backupHint = computed(() => {
  if (!backupLive.value) return 'Grid submeter status is stale'
  if (backupActive.value) return 'Submeter is supplying the grid measurement'
  if (!props.gridBackup?.available) return 'Grid submeter unavailable'
  return props.gridBackup.enabled
    ? 'Grid submeter ready as backup'
    : 'Grid submeter detected; backup control disabled'
})

/** Sticky last-known for a numeric prop: hold on null/undefined, accept explicit 0. */
function useHeldNumber(
  get: () => number | null | undefined,
  available: () => boolean | undefined = () => undefined
) {
  const held = ref<number | undefined>()
  watch(
    () => [get(), available()] as const,
    ([v, valid]) => {
      if (valid === false) {
        held.value = undefined
      } else if (v !== null && v !== undefined && Number.isFinite(v)) {
        held.value = v
      }
    },
    { immediate: true, flush: 'sync' }
  )
  return computed(() => (available() === false ? undefined : holdNumber(get(), held.value)))
}

function formatGridPower(value: number | undefined): string {
  return value === undefined ? '—' : formatPower(value)
}

const gtH = useHeldNumber(
  () => props.gt,
  () => (props.gridL1Available === false && props.gridL2Available === false ? false : undefined)
)
const g1H = useHeldNumber(
  () => props.g1,
  () => props.gridL1Available
)
const g2H = useHeldNumber(
  () => props.g2,
  () => props.gridL2Available
)
const ttH = useHeldNumber(() => props.tt)
const t1H = useHeldNumber(() => props.t1)
const t2H = useHeldNumber(() => props.t2)
const solarTotalH = useHeldNumber(() => props.solarTotal)
const mpptTotalH = useHeldNumber(() => props.mpptTotal)
const pvInvertersTotalH = useHeldNumber(() => props.pvInvertersTotal)
const batterySocH = useHeldNumber(() => props.batterySoc)
const batteryPowerH = useHeldNumber(() => props.batteryPower)
const batteryVoltageH = useHeldNumber(() => props.batteryVoltage)
const batteryCurrentH = useHeldNumber(() => props.batteryCurrent)
const setpointH = useHeldNumber(() => props.setpoint)
</script>
