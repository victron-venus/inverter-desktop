<template>
  <div class="row g-2 mb-2">
    <div class="col-md-6">
      <div class="card h-100">
        <div class="card-header"><i class="fas fa-battery-three-quarters me-2"></i>Batteries</div>
        <div class="card-body py-1" style="font-size:0.75rem">
          <div class="d-flex flex-wrap gap-2">
            <div v-for="bat in batteries" :key="bat.name" class="flex-fill subsection" style="min-width:140px">
              <div class="fw-bold mb-1" style="font-size:0.65rem;color:var(--text-dim)">{{ bat.name }}</div>
              <div class="d-flex justify-content-between subsection-value">
                <span>{{ bat.voltage.toFixed(2) }}V</span>
                <span v-if="bat.current !== undefined">{{ bat.current.toFixed(1) }}A</span>
                <span v-if="bat.power !== undefined">{{ Math.floor(bat.power) }}W</span>
              </div>
              <div class="d-flex justify-content-between mt-1">
                <span class="fw-bold" :style="{color: bat.soc > 50 ? '#7ed321' : bat.soc > 20 ? '#f5a623' : '#e74c3c'}">{{ bat.soc.toFixed(1) }}%</span>
                <span style="color:var(--text-dim);text-align:right">{{ bat.state }}{{ bat.timeToGo ? ' · ' + bat.timeToGo : '' }}</span>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
    <div class="col-md-6">
      <div class="card h-100">
        <div class="card-header"><i class="fas fa-solar-panel me-2"></i>Solar Production</div>
        <div class="card-body py-1" style="font-size:0.75rem">
          <div class="d-flex flex-wrap gap-2">
            <div v-for="src in solarSources" :key="src.name" class="flex-fill subsection" style="min-width:100px">
              <div class="fw-bold mb-1" style="font-size:0.65rem;color:var(--text-dim)">{{ src.name }}</div>
              <div v-if="src.pvVoltage" class="subsection-value" style="color:var(--solar)">{{ src.pvVoltage.toFixed(2) }}V</div>
              <div v-if="src.current" class="subsection-value">{{ src.current.toFixed(1) }}A</div>
              <div class="fw-bold" style="color:var(--solar)">{{ Math.floor(src.power) }}W</div>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
defineProps<{
  batteries: Array<{ name: string; voltage: number; current?: number; power?: number; soc: number; state: string; timeToGo?: string }>
  solarSources: Array<{ name: string; pvVoltage?: number; current?: number; power: number }>
}>()
</script>
