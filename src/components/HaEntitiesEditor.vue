<template>
  <div class="flex flex-col gap-2">
    <div class="flex items-center justify-between px-1">
      <h3 class="classic-subsection-title">Home Buttons</h3>
      <button
        type="button"
        @click="$emit('add')"
        class="text-[10px] font-semibold text-accent hover:opacity-80 flex items-center gap-1"
      >
        <Plus :size="12" /> Add Button
      </button>
    </div>

    <div
      v-if="haEntitiesList.length === 0"
      class="py-4 text-center border border-dashed border-black/10 dark:border-white/10 rounded-lg text-[11px] text-muted bg-black/[0.015] dark:bg-white/[0.02]"
    >
      No home buttons configured.
    </div>

    <div class="flex flex-col gap-1.5">
      <div
        v-for="(entity, index) in haEntitiesList"
        :key="entity.id || `home-${index}`"
        class="classic-inset !rounded-lg p-2 flex flex-col gap-2"
      >
        <div class="flex items-center gap-2">
          <div class="flex-1 grid grid-cols-2 gap-2">
            <div class="flex flex-col gap-0.5">
              <label :for="'ha-label-' + index" class="classic-label px-1">Label</label>
              <input
                :id="'ha-label-' + index"
                v-model="entity.label"
                type="text"
                class="classic-input !h-7 w-full"
                placeholder="Name"
              />
            </div>
            <div class="flex flex-col gap-0.5">
              <label :for="'ha-entity-' + index" class="classic-label px-1">Entity ID</label>
              <EntityAutocompleteInput
                :id="'ha-entity-' + index"
                v-model="entity.entity"
                :entities="discoveredEntities"
                placeholder="switch.xxx"
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
              :disabled="index === haEntitiesList.length - 1"
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

        <div
          class="flex items-center gap-4 px-1 border-t border-black/[0.04] dark:border-white/[0.06] pt-1.5"
        >
          <label class="flex items-center gap-2 cursor-pointer group">
            <input type="checkbox" v-model="entity.enabled" class="sr-only peer" />
            <div
              class="w-6 h-3.5 bg-black/10 dark:bg-white/10 peer-checked:bg-accent rounded-full relative transition-colors after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:rounded-full after:h-2.5 after:w-2.5 after:transition-all peer-checked:after:translate-x-2.5"
            ></div>
            <span
              class="text-[10px] font-semibold text-muted group-hover:text-accent transition-colors"
              >Active</span
            >
          </label>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { Plus, Trash2, ChevronUp, ChevronDown } from '@lucide/vue'
import EntityAutocompleteInput from './EntityAutocompleteInput.vue'

defineProps<{
  haEntitiesList: Array<{
    id: string
    label: string
    entity: string
    domain: string
    enabled: boolean
  }>
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
