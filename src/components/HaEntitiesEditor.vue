<template>
  <div class="flex flex-col gap-2">
    <div class="flex items-center justify-between px-1">
      <h3 class="text-[11px] font-bold uppercase tracking-widest text-slate-600">Home Buttons</h3>
      <button
        @click="$emit('add')"
        class="text-[10px] font-bold text-accent hover:underline flex items-center gap-1 uppercase"
      >
        <Plus :size="12" /> Add Button
      </button>
    </div>

    <div
      v-if="haEntitiesList.length === 0"
      class="py-4 text-center border border-dashed border-slate-200 dark:border-slate-800 rounded text-[11px] text-slate-600 bg-slate-50/30 dark:bg-slate-900/30"
    >
      No home buttons configured.
    </div>

    <div class="flex flex-col gap-1.5">
      <div
        v-for="(entity, index) in haEntitiesList"
        :key="entity.id || `home-${index}`"
        class="classic-card !rounded p-2 flex flex-col gap-2 bg-white dark:bg-slate-900"
      >
        <div class="flex items-center gap-2">
          <div class="flex-1 grid grid-cols-2 gap-2">
            <div class="flex flex-col gap-0.5">
              <label :for="'ha-label-' + index" class="text-[9px] font-bold uppercase text-slate-600 px-1">Label</label>
              <input
                :id="'ha-label-' + index"
                v-model="entity.label"
                type="text"
                class="classic-input !h-7 w-full"
                placeholder="Name"
              />
            </div>
            <div class="flex flex-col gap-0.5">
              <label :for="'ha-entity-' + index" class="text-[9px] font-bold uppercase text-slate-600 px-1">Entity ID</label>
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
              @click="$emit('move-up', index)"
              :disabled="index === 0"
              class="p-1 rounded hover:bg-slate-100 dark:hover:bg-slate-800 disabled:opacity-20 text-slate-600"
            >
              <ChevronUp :size="14" />
            </button>
            <button
              @click="$emit('move-down', index)"
              :disabled="index === haEntitiesList.length - 1"
              class="p-1 rounded hover:bg-slate-100 dark:hover:bg-slate-800 disabled:opacity-20 text-slate-600"
            >
              <ChevronDown :size="14" />
            </button>
            <button
              @click="$emit('remove', index)"
              class="p-1 rounded hover:bg-red-50 dark:hover:bg-red-950/20 hover:text-red-500 transition-colors text-slate-300"
            >
              <Trash2 :size="14" />
            </button>
          </div>
        </div>

        <div
          class="flex items-center gap-4 px-1 border-t border-slate-50 dark:border-slate-800/50 pt-1.5"
        >
          <label class="flex items-center gap-2 cursor-pointer group">
            <input type="checkbox" v-model="entity.enabled" class="sr-only peer" />
            <div
              class="w-6 h-3.5 bg-slate-200 dark:bg-slate-800 peer-checked:bg-accent rounded-full relative transition-colors after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:rounded-full after:h-2.5 after:w-2.5 after:transition-all peer-checked:after:translate-x-2.5 shadow-inner"
            ></div>
            <span
              class="text-[9px] font-bold uppercase text-slate-600 group-hover:text-accent transition-colors"
              >Active</span
            >
          </label>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { Plus, Trash2, ChevronUp, ChevronDown } from 'lucide-vue-next'
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
