<template>
  <div class="col-md-4">
    <!-- EV -->
    <div class="card mb-2" v-if="features?.ev !== false && (parseFloat(evCharging) > 0 || parseFloat(evPower) > 0)">
      <div class="card-header"><i class="fas fa-car me-2"></i>EV</div>
      <div class="card-body py-1">
        <div class="d-flex justify-content-between">
          <div><div class="stat-value text-solar">{{ evCharging }}</div><div class="stat-sub">Charging</div></div>
          <div class="text-center"><div class="stat-value" style="color:#9e9e9e">{{ evPower }}</div><div class="stat-sub">VUE</div></div>
          <div class="text-end"><div class="stat-value text-accent">{{ Math.floor(carSoc || 0) }}%</div><div class="stat-sub">SoC</div></div>
        </div>
      </div>
    </div>
    <!-- Water -->
    <div class="card mb-2" v-if="features?.water !== false">
      <div class="card-header"><i class="fas fa-faucet me-2"></i>Water</div>
      <div class="card-body py-1">
        <div class="d-flex justify-content-between align-items-center">
          <div class="fw-bold" :style="{color: waterValve ? '#f44336' : '#4caf50'}">{{ waterLevel || 0 }} cm</div>
          <div class="d-flex gap-1">
            <v-btn
              outlined
              :class="['custom-3d', pumpSwitch ? 'state-on' : 'state-off']"
              size="small"
              @click="$emit('send', 'toggle', {entity: pumpSwitchEntity})"
            >
              PUMP
            </v-btn>
            <v-btn
              outlined
              :class="['custom-3d', waterValve ? 'state-on' : 'state-off']"
              size="small"
              @click="$emit('send', 'toggle', {entity: waterValveEntity})"
            >
              VALVE
            </v-btn>
          </div>
        </div>
      </div>
      </div>
      <!-- Dishwasher -->
      <div class="card mb-2" v-if="features?.dishwasher !== false && dishwasherRunning">
      <div class="card-header"><i class="fas fa-utensils me-2"></i>Dishwasher</div>
      <div class="card-body py-1">
        <div class="d-flex justify-content-between align-items-center">
          <div class="fw-bold text-success">Running</div>
          <div>{{ formatDuration(dishwasherDuration) }}</div>
        </div>
      </div>
    </div>
    <!-- Washer -->
    <div class="card mb-2" v-if="features?.washer !== false && (((washerTime || 0) > 0) || washerPower)">
      <div class="card-header"><i class="fas fa-soap me-2"></i>Washer</div>
      <div class="card-body py-1">
        <div class="d-flex justify-content-between align-items-center">
          <div class="fw-bold">{{ formatDuration(washerTime) }}</div>
          <v-btn
            outlined
            :class="['custom-3d', washerPower ? 'state-on' : 'state-off']"
            size="small"
            disabled
          >
            PWR
          </v-btn>
        </div>
      </div>
    </div>
    <!-- Dryer -->
    <div class="card mb-2" v-if="features?.dryer !== false && (((dryerTime || 0) > 0) || dryerPower)">
      <div class="card-header"><i class="fas fa-wind me-2"></i>Dryer</div>
      <div class="card-body py-1">
        <div class="d-flex justify-content-between align-items-center">
          <div class="fw-bold">{{ formatDuration(dryerTime) }}</div>
          <v-btn
            outlined
            :class="['custom-3d', dryerPower ? 'state-on' : 'state-off']"
            size="small"
            disabled
          >
            PWR
          </v-btn>
        </div>
      </div>
    </div>
    <!-- Home -->
    <div class="card" v-if="features?.ha !== false && homeButtons.length > 0">
      <div class="card-header"><i class="fas fa-home me-2"></i>Home</div>
      <div class="card-body py-1">
        <div class="d-flex gap-1 flex-wrap">
          <v-btn
            v-for="btn in homeButtons"
            :key="btn.id"
            outlined
            :class="['custom-3d', (buttonStates[btn.id] === 'on') ? 'state-on' : 'state-off']"
            size="small"
            @click="$emit('send', 'toggle', {entity: btn.entity})"
          >
            {{ btn.label }}
          </v-btn>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { formatDuration } from '../utils'

defineProps<{
  features?: Record<string, boolean>
  evCharging: string
  evPower: string
  carSoc?: number
  waterLevel?: number
  waterValve?: boolean
  pumpSwitch?: boolean
  pumpSwitchEntity: string
  waterValveEntity: string
  dishwasherRunning?: boolean
  dishwasherDuration?: number
  washerTime?: number
  washerPower?: boolean
  dryerTime?: number
  dryerPower?: boolean
  homeButtons: Array<{ id: string; label: string; entity: string }>
  buttonStates: Record<string, string>
}>()
defineEmits<{
  send: [action: string, payload?: any]
}>()
</script>
