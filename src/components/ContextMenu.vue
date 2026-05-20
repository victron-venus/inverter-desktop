<template>
  <div
    v-if="show"
    class="context-menu"
    :style="{ left: x + 'px', top: y + 'px' }"
    @click.stop
  >
    <div class="context-menu-item" @click="$emit('open-config')">Configuration</div>
  </div>
</template>

<script setup lang="ts">
defineProps<{
  show: boolean
  x: number
  y: number
}>()
defineEmits<{
  'open-config': []
}>()
</script>
