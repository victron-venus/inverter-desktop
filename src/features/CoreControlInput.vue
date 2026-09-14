<template>
  <select :id="id" class="classic-input !h-7 w-full" :value="modelValue" @change="onChange">
    <option value="" disabled>Select inverter control</option>
    <option
      v-for="control in DEFAULT_INVERTER_CONTROLS"
      :key="control.entity"
      :value="control.entity"
    >
      {{ control.label }}
    </option>
  </select>
</template>
<script setup lang="ts">
import { DEFAULT_INVERTER_CONTROLS } from '../inverterControl'
defineProps<{ id?: string; modelValue: string }>()
const emit = defineEmits<{ 'update:modelValue': [value: string] }>()
function onChange(event: Event) {
  emit('update:modelValue', (event.target as HTMLSelectElement).value)
}
</script>
