<template>
  <fieldset
    v-if="schema.type === 'object'"
    class="flex flex-col gap-2 min-w-0"
    :disabled="disabled"
  >
    <legend v-if="schema.title" class="classic-label">{{ schema.title }}</legend>
    <template v-for="(field, key) in schema.properties" :key="key">
      <PluginJsonField
        v-if="!('const' in field)"
        :schema="{ ...field, title: field.title ?? key }"
        :model-value="objectValue[key] ?? initialJsonValue(field)"
        :choices="choices"
        :disabled="disabled"
        :secret="secret"
        @update:model-value="updateProperty(key, $event)"
      />
    </template>
    <template v-if="typeof schema.additionalProperties === 'object'">
      <div
        v-for="(value, key) in additionalEntries"
        :key="key"
        class="classic-inset p-2 flex gap-2 items-start"
      >
        <label class="flex flex-col gap-1 min-w-0 flex-1"
          ><span class="classic-label">Key</span
          ><input
            class="classic-input"
            :value="key"
            maxlength="128"
            :disabled="disabled"
            @change="renameProperty(key, $event.target as HTMLInputElement)"
        /></label>
        <PluginJsonField
          class="flex-1"
          :schema="schema.additionalProperties"
          :model-value="value"
          :choices="choices"
          :disabled="disabled"
          :secret="secret"
          @update:model-value="updateProperty(key, $event)"
        />
        <UiButton :disabled="disabled" @click="removeProperty(key)">Remove</UiButton>
      </div>
      <UiButton
        class="self-start"
        :disabled="disabled || Object.keys(objectValue).length >= (schema.maxProperties ?? 128)"
        @click="addProperty"
        >Add entry</UiButton
      >
    </template>
  </fieldset>
  <fieldset
    v-else-if="schema.type === 'array' && schema.items"
    class="flex flex-col gap-2 min-w-0"
    :disabled="disabled"
  >
    <legend v-if="schema.title" class="classic-label">{{ schema.title }}</legend>
    <div
      v-for="(value, index) in arrayValue"
      :key="index"
      class="classic-inset p-2 flex flex-col gap-1"
    >
      <div class="flex justify-end gap-1">
        <UiButton
          :disabled="disabled || arrayValue.length <= (schema.minItems ?? 0)"
          @click="removeItem(index)"
          >Remove</UiButton
        >
      </div>
      <PluginJsonField
        :schema="schema.items"
        :model-value="value"
        :choices="choices"
        :disabled="disabled"
        :secret="secret"
        @update:model-value="updateItem(index, $event)"
      />
    </div>
    <UiButton
      class="self-start"
      :disabled="disabled || arrayValue.length >= (schema.maxItems ?? 128)"
      @click="emit('update:modelValue', [...arrayValue, initialJsonValue(schema.items)])"
      >Add</UiButton
    >
  </fieldset>
  <label v-else class="flex flex-col gap-1 min-w-0">
    <span v-if="schema.title" class="classic-label">{{ schema.title }}</span>
    <span v-if="schema.description" class="text-[11px] text-muted">{{ schema.description }}</span>
    <select
      v-if="schema.enum"
      class="classic-input"
      :value="String(modelValue ?? '')"
      :disabled="disabled"
      @change="selectEnum(($event.target as HTMLSelectElement).value)"
    >
      <option v-for="option in schema.enum" :key="String(option)" :value="String(option)">
        {{ option }}
      </option>
    </select>
    <input
      v-else-if="schema.type === 'boolean'"
      type="checkbox"
      class="self-start"
      :checked="modelValue === true"
      :disabled="disabled"
      @change="emit('update:modelValue', ($event.target as HTMLInputElement).checked)"
    />
    <PluginChoiceInput
      v-else-if="schema.type === 'string'"
      :model-value="String(modelValue ?? '')"
      :options="options"
      :multiple="schema['x-options-multiple']"
      :secret="secret"
      :disabled="disabled"
      :label="schema.title"
      :max-length="Math.min(24576, (schema.maxLength ?? 24576) * 2)"
      @update:model-value="emit('update:modelValue', $event)"
    />
    <input
      v-else
      type="number"
      class="classic-input"
      :value="modelValue"
      :step="schema.type === 'integer' ? 1 : 'any'"
      :min="schema.minimum"
      :max="schema.maximum"
      :disabled="disabled"
      @input="updateNumber($event.target as HTMLInputElement)"
    />
  </label>
</template>
<script setup lang="ts">
import { computed } from 'vue'
import UiButton from '../../../components/UiButton.vue'
import PluginChoiceInput from './PluginChoiceInput.vue'
import { fieldChoices, initialJsonValue } from './jsonEditor'
import type { JsonEditorSchema, PluginSettingsChoices } from './types'
const props = defineProps<{
  schema: JsonEditorSchema
  modelValue: unknown
  choices: PluginSettingsChoices
  disabled?: boolean
  secret?: boolean
}>()
const emit = defineEmits<{ 'update:modelValue': [value: unknown] }>()
const objectValue = computed<Record<string, unknown>>(() =>
  props.modelValue && typeof props.modelValue === 'object' && !Array.isArray(props.modelValue)
    ? (props.modelValue as Record<string, unknown>)
    : {}
)
const arrayValue = computed<unknown[]>(() =>
  Array.isArray(props.modelValue) ? props.modelValue : []
)
const additionalEntries = computed(() =>
  Object.fromEntries(
    Object.entries(objectValue.value).filter(([key]) => !hasOwn(props.schema.properties ?? {}, key))
  )
)
const options = computed(() =>
  fieldChoices(props.schema['x-options-source'], props.schema['x-options-prefixes'], props.choices)
)
function hasOwn(value: object, key: string) {
  return Object.keys(value).includes(key)
}
function updateNumber(input: HTMLInputElement) {
  const value = Number(input.value)
  const valid =
    input.value !== '' &&
    Number.isFinite(value) &&
    (props.schema.type !== 'integer' ||
      (Number.isSafeInteger(value) && /^[+-]?\d+$/.test(input.value)))
  input.setCustomValidity(valid ? '' : 'Enter a valid number.')
  if (valid) emit('update:modelValue', value)
}
function updateProperty(key: string, value: unknown) {
  emit('update:modelValue', { ...objectValue.value, [key]: value })
}
function removeProperty(key: string) {
  const next = { ...objectValue.value }
  delete next[key]
  emit('update:modelValue', next)
}
function renameProperty(key: string, input: HTMLInputElement) {
  const nextKey = input.value
  if (
    !nextKey ||
    ['__proto__', 'prototype', 'constructor'].includes(nextKey) ||
    (nextKey !== key && hasOwn(objectValue.value, nextKey))
  ) {
    input.value = key
    return
  }
  emit(
    'update:modelValue',
    Object.fromEntries(
      Object.entries(objectValue.value).map(([current, value]) => [
        current === key ? nextKey : current,
        value,
      ])
    )
  )
}
function addProperty() {
  const schema = props.schema.additionalProperties
  if (!schema || typeof schema !== 'object') return
  let key = 'new'
  let suffix = 2
  while (hasOwn(objectValue.value, key)) key = `new${suffix++}`
  updateProperty(key, initialJsonValue(schema))
}
function updateItem(index: number, value: unknown) {
  emit(
    'update:modelValue',
    arrayValue.value.map((item, i) => (i === index ? value : item))
  )
}
function removeItem(index: number) {
  emit(
    'update:modelValue',
    arrayValue.value.filter((_, i) => i !== index)
  )
}
function selectEnum(value: string) {
  const selected = props.schema.enum?.find((item) => String(item) === value)
  if (selected !== undefined) emit('update:modelValue', selected)
}
</script>
