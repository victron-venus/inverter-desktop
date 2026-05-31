<template>
  <div class="relative w-full">
    <input
      :value="modelValue"
      @input="handleInput"
      @focus="handleFocus"
      @blur="handleBlur"
      type="text"
      class="classic-input !h-7 w-full"
      :placeholder="placeholder"
    />
    
    <div v-if="showSuggestions && filteredEntities.length > 0" 
         class="absolute z-50 left-0 right-0 mt-1 max-h-48 overflow-y-auto bg-white dark:bg-slate-900 border border-slate-200 dark:border-slate-800 rounded shadow-xl">
      <div
        v-for="entity in filteredEntities"
        :key="entity.entity_id"
        @mousedown.prevent="selectEntity(entity.entity_id)"
        class="px-2 py-1.5 hover:bg-slate-100 dark:hover:bg-slate-800 cursor-pointer border-b border-slate-50 dark:border-slate-800/50 last:border-0"
      >
        <div class="text-[10px] font-bold text-slate-700 dark:text-slate-200 truncate">{{ entity.friendly_name }}</div>
        <div class="text-[8px] text-slate-400 font-mono truncate">{{ entity.entity_id }}</div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue'

const props = defineProps<{
  modelValue: string
  placeholder?: string
  entities: Array<{ entity_id: string; friendly_name: string; domain: string }>
}>()

const emit = defineEmits(['update:modelValue', 'focus'])

const isFocused = ref(false)
const showSuggestions = ref(false)

const filteredEntities = computed(() => {
  const query = props.modelValue.toLowerCase()
  if (!query) return []
  
  return props.entities
    .filter(e => 
      e.entity_id.toLowerCase().includes(query) || 
      e.friendly_name.toLowerCase().includes(query)
    )
    .slice(0, 15) // Limit results
})

function handleInput(e: Event) {
  const val = (e.target as HTMLInputElement).value
  emit('update:modelValue', val)
  showSuggestions.value = true
}

function handleFocus() {
  isFocused.value = true
  showSuggestions.value = true
  emit('focus')
}

function handleBlur() {
  isFocused.value = false
  // Delay hiding to allow mousedown to trigger on suggestions
  setTimeout(() => {
    showSuggestions.value = false
  }, 150)
}

function selectEntity(entityId: string) {
  emit('update:modelValue', entityId)
  showSuggestions.value = false
}
</script>
