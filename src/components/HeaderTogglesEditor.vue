<template>
  <div class="flex flex-col gap-2">
    <div class="flex items-center justify-between px-1">
      <h3 class="classic-subsection-title">Header Toggles</h3>
      <button
        type="button"
        @click="$emit('add')"
        class="text-[10px] font-semibold text-accent hover:opacity-80 flex items-center gap-1"
      >
        <Plus :size="12" /> Add Toggle
      </button>
    </div>

    <div
      v-if="headerTogglesList.length === 0"
      class="py-4 text-center border border-dashed border-black/10 dark:border-white/10 rounded-lg text-[11px] text-muted bg-black/[0.015] dark:bg-white/[0.02]"
    >
      No header toggles configured.
    </div>

    <div class="flex flex-col gap-1.5">
      <div
        v-for="(toggle, index) in headerTogglesList"
        :key="toggle.id || `toggle-${index}`"
        class="classic-inset !rounded-lg p-2 flex items-center gap-2"
      >
        <div class="flex-1 grid grid-cols-2 gap-2">
          <div class="flex flex-col gap-0.5">
            <label :for="'ht-label-' + index" class="classic-label px-1">Label</label>
            <input
              :id="'ht-label-' + index"
              v-model="toggle.label"
              type="text"
              class="classic-input !h-7 w-full"
              placeholder="Name"
            />
          </div>
          <div class="flex flex-col gap-0.5">
            <label :for="'ht-entity-' + index" class="classic-label px-1">Entity ID</label>
            <EntityAutocompleteInput
              :id="'ht-entity-' + index"
              v-model="toggle.entity"
              :entities="discoveredEntities"
              placeholder="input_boolean.xxx"
              @focus="$emit('focus-entity')"
            />
          </div>
        </div>

        <div class="flex items-center gap-0.5 pt-3">
          <button
            type="button"
            @click="$emit('move-up', index)"
            :disabled="index === 0"
            class="p-1 rounded-md hover:bg-black/[0.04] dark:hover:bg-white/[0.06] disabled:opacity-20 text-muted"
          >
            <ChevronUp :size="14" />
          </button>
          <button
            type="button"
            @click="$emit('move-down', index)"
            :disabled="index === headerTogglesList.length - 1"
            class="p-1 rounded-md hover:bg-black/[0.04] dark:hover:bg-white/[0.06] disabled:opacity-20 text-muted"
          >
            <ChevronDown :size="14" />
          </button>
          <button
            type="button"
            @click="$emit('remove', index)"
            class="p-1 rounded-md hover:bg-red-50 dark:hover:bg-red-950/20 hover:text-consumption transition-colors text-muted/50"
          >
            <Trash2 :size="14" />
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { Plus, Trash2, ChevronUp, ChevronDown } from '@lucide/vue'
import EntityAutocompleteInput from './EntityAutocompleteInput.vue'

defineProps<{
  headerTogglesList: Array<{ id: string; label: string; entity: string }>
  discoveredEntities: Array<{ entity_id: string; friendly_name: string; domain: string }>
  entityRules: ((v: string) => boolean | string)[]
}>()

defineEmits<{
  add: []
  remove: [index: number]
  'move-up': [index: number]
  'move-down': [index: number]
  'focus-entity': []
}>()
</script>
