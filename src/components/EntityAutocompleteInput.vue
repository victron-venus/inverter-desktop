<template>
  <div class="relative w-full">
    <input
      :id="id"
      :value="modelValue"
      @input="handleInput"
      @focus="handleFocus"
      @blur="handleBlur"
      type="text"
      class="classic-input !h-7 w-full"
      :placeholder="placeholder"
    />

    <div
      v-if="showSuggestions && filteredEntities.length > 0"
      class="absolute z-50 left-0 right-0 mt-1 max-h-48 overflow-y-auto apple-card !rounded-lg"
    >
      <div
        v-for="entity in filteredEntities"
        :key="entity.entity_id"
        @mousedown.prevent="selectEntity(entity.entity_id)"
        class="row-hover px-2.5 py-1.5 cursor-pointer border-b border-black/[0.04] dark:border-white/[0.05] last:border-0"
      >
        <div class="text-[10px] font-semibold text-main truncate">
          {{ entity.friendly_name }}
        </div>
        <div class="text-[8px] text-muted font-mono truncate">{{ entity.entity_id }}</div>
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
  id?: string
}>()

const emit = defineEmits(['update:modelValue', 'focus'])

const isFocused = ref(false)
const showSuggestions = ref(false)

const filteredEntities = computed(() => {
  const query = props.modelValue.toLowerCase()
  if (!query) return []

  return props.entities
    .filter(
      (e) =>
        e.entity_id.toLowerCase().includes(query) || e.friendly_name.toLowerCase().includes(query)
    )
    .slice(0, 15)
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
  setTimeout(() => {
    showSuggestions.value = false
  }, 150)
}

function selectEntity(entityId: string) {
  emit('update:modelValue', entityId)
  showSuggestions.value = false
}
</script>
