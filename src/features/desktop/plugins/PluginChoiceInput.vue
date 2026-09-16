<template>
  <div class="flex flex-col gap-1">
    <input
      :id="id"
      :value="modelValue"
      :type="secret ? 'password' : 'text'"
      :list="options.length && !multiple ? listId : undefined"
      :disabled="disabled"
      :maxlength="maxLength"
      :aria-label="label"
      class="classic-input min-w-0 w-full"
      autocomplete="off"
      @input="emit('update:modelValue', ($event.target as HTMLInputElement).value)"
    />
    <datalist v-if="options.length && !multiple" :id="listId">
      <option v-for="option in options" :key="option.value" :value="option.value">
        {{ option.label }}
      </option>
    </datalist>
    <div
      v-if="multiple && options.length"
      class="classic-inset max-h-36 overflow-y-auto flex flex-col gap-1"
    >
      <label
        v-for="option in options"
        :key="option.value"
        class="flex items-center gap-1 text-[11px]"
      >
        <input
          type="checkbox"
          :checked="selected.includes(option.value)"
          :disabled="disabled"
          @change="select(option.value, ($event.target as HTMLInputElement).checked)"
        />
        <span class="truncate" :title="option.value"
          >{{ option.label }} <span class="text-muted">{{ option.value }}</span></span
        >
      </label>
    </div>
  </div>
</template>
<script setup lang="ts">
import { computed, useId } from 'vue'
const props = defineProps<{
  modelValue: string
  options: Array<{ value: string; label: string }>
  multiple?: boolean
  disabled?: boolean
  label?: string
  id?: string
  secret?: boolean
  maxLength?: number
}>()
const emit = defineEmits<{ 'update:modelValue': [value: string] }>()
const listId = useId()
const selected = computed(() =>
  props.modelValue
    .split(/[,\n]/)
    .map((item) => item.trim())
    .filter(Boolean)
)
function select(value: string, checked: boolean) {
  emit(
    'update:modelValue',
    (checked
      ? [...new Set([...selected.value, value])]
      : selected.value.filter((item) => item !== value)
    ).join(',')
  )
}
</script>
