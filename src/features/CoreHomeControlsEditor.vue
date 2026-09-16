<template>
  <div class="flex flex-col gap-2">
    <div class="flex items-center justify-between">
      <h3 class="classic-subsection-title">Home controls</h3>
      <UiButton @click="controls.addHomeControl">Add control</UiButton>
    </div>
    <div
      v-for="(control, index) in controls.haEntitiesList.value"
      :key="index"
      class="classic-inset p-2 flex flex-wrap items-center gap-2"
    >
      <label class="flex flex-col gap-1 flex-1"
        ><span class="classic-label">Label</span
        ><input v-model="control.label" class="classic-input"
      /></label>
      <label :for="`${prefix}-${index}`" class="flex flex-col gap-1 flex-1"
        ><span class="classic-label">Inverter control</span
        ><CoreControlInput :id="`${prefix}-${index}`" v-model="control.entity"
      /></label>
      <label class="flex items-center gap-1 text-[11px]"
        ><input v-model="control.enabled" type="checkbox" />Enabled</label
      >
      <UiButton
        :disabled="index === 0"
        aria-label="Move up"
        @click="controls.moveHomeControl(index, -1)"
        >↑</UiButton
      >
      <UiButton
        :disabled="index === controls.haEntitiesList.value.length - 1"
        aria-label="Move down"
        @click="controls.moveHomeControl(index, 1)"
        >↓</UiButton
      >
      <UiButton @click="controls.removeHomeControl(index)">Remove</UiButton>
    </div>
  </div>
</template>
<script setup lang="ts">
import { useId } from 'vue'
import CoreControlInput from './CoreControlInput.vue'
import UiButton from '../components/UiButton.vue'
import type { useCoreControlsConfig } from './coreControlsConfig'
const prefix = useId()
defineProps<{ controls: ReturnType<typeof useCoreControlsConfig> }>()
</script>
