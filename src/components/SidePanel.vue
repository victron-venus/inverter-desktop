<template>
  <div class="flex flex-col gap-1.5 h-full side-panel">
    <!-- EV Section -->
    <div v-if="showEv !== false && evSectionVisible" class="classic-card">
      <div class="classic-header flex items-center gap-1.5">
        <Car :size="10" /> {{ $t('sections.ev') }}
      </div>
      <div class="p-1 grid grid-cols-3 gap-1 text-center">
        <div v-if="carChargingPower != null">
          <div class="text-[15px] font-bold text-solar leading-none tabular tracking-tight">
            {{ (carChargingPower / 1000).toFixed(1) }}kW
          </div>
          <div class="text-[9px] text-muted font-semibold mt-0.5">
            {{ $t('sections.charging') }}
          </div>
        </div>
        <div v-if="evChargingPower != null">
          <div class="text-[15px] font-bold text-muted leading-none tabular tracking-tight">
            {{ (evChargingPower / 1000).toFixed(1) }}kW
          </div>
          <div class="text-[9px] text-muted font-semibold mt-0.5">
            {{ $t('sections.evcharger') }}
          </div>
        </div>
        <div v-if="carSoc != null">
          <div class="text-[15px] font-bold text-accent leading-none tabular tracking-tight">
            {{ Math.floor(carSoc) }}%
          </div>
          <div class="text-[9px] text-muted font-semibold mt-0.5">
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
      <div class="p-1 flex items-center justify-between gap-2">
        <div
          v-if="waterLevel != null"
          class="text-[16px] font-bold tabular tracking-tight"
          :class="waterValve === true ? 'text-consumption' : 'text-battery'"
        >
          {{ Math.round(waterLevel) }}%
        </div>
        <div class="flex gap-0.5 items-center">
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
      v-if="showHomeSection !== false && homeButtons.length > 0"
      class="classic-card flex flex-col flex-1 min-h-0"
    >
      <div class="classic-header flex items-center gap-1.5 shrink-0">
        <HomeIcon :size="10" /> {{ $t('sections.home') }}
      </div>
      <div class="home-btn-scroll p-1 overflow-y-auto max-h-[300px] min-h-0">
        <div class="home-btn-grid">
          <UiButton
            v-for="btn in homeButtons"
            :key="btn.id"
            variant="tile"
            class="home-btn-tile"
            toggle
            :active="(btn.state ?? buttonStates[btn.id]) === 'on'"
            :unavailable="(btn.state ?? buttonStates[btn.id]) === 'unavailable'"
            :disabled="
              btn.disabled || (controlsConnected === false && !isInverterControlFlag(btn.entity))
            "
            :loading="btn.pending"
            @click="activate(btn)"
          >
            <component
              :is="btn.icon ?? getControlIcon?.(btn.entity, btn.label)"
              v-if="btn.icon || getControlIcon?.(btn.entity, btn.label)"
              :size="12"
              class="home-tile-icon opacity-70 shrink-0"
            />
            <span class="home-tile-label">{{ getControlLabel?.(btn.label) ?? btn.label }}</span
            ><span
              v-if="btn.failed"
              role="alert"
              title="Action unconfirmed; check the current state before retrying."
              >!</span
            >
          </UiButton>
        </div>
      </div>
    </div>

    <slot />
  </div>
</template>
<script setup lang="ts">
import type { Component } from 'vue'
import type { DashboardControlView } from '../dashboardControlView'
import { Car, Droplets, Home as HomeIcon } from '@lucide/vue'
import UiButton from './UiButton.vue'
import { useI18n } from 'vue-i18n'
import { isInverterControlFlag } from '../inverterControl'
const props = defineProps<{
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
  getControlLabel?: (label: string) => string
  getControlIcon?: (entity: string, label: string) => Component | null
  homeButtons: DashboardControlView[]
  buttonStates: Record<string, string>
  showHomeSection?: boolean
  controlsConnected?: boolean
}>()
const emit = defineEmits<{ send: [action: string, payload?: Record<string, unknown>] }>()
const { t: $t } = useI18n()
function activate(control: DashboardControlView) {
  if (control.disabled || control.pending) return
  if (control.activate) control.activate()
  else emit('send', 'toggle', { entity: control.entity })
}
function onValveClick() {
  if (props.waterValve === false && !window.confirm('Open city water valve?')) return
  emit('send', 'water_mode', { which: 'valve', mode: props.waterValve ? 2 : 1 })
}
</script>
