<template>
  <div class="row g-2 mb-2" v-if="sortedLoads.length">
    <div class="col-12">
      <div class="card">
        <div class="card-header">Loads</div>
        <div class="card-body py-1" id="loads">
          <div class="loads-table">
            <div class="loads-row" v-for="[name, val] in sortedLoads" :key="name">
              <span class="loads-name">{{ name }}</span>
              <span class="loads-value">{{ Math.floor(val) }}W</span>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
defineProps<{
  sortedLoads: Array<[string, number]>
}>()
</script>
