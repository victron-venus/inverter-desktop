<template>
  <div class="flex flex-col gap-2">
    <template v-if="secret && !replacing">
      <UiButton class="self-start" :disabled="disabled" @click="begin">{{
        $t('plugins.manager.replaceSecret')
      }}</UiButton>
    </template>
    <template v-else-if="invalid">
      <p role="alert" class="text-[12px] text-consumption">
        {{ $t('plugins.manager.invalidStructuredSetting') }}
      </p>
    </template>
    <PluginJsonField
      v-else
      :schema="schema"
      :model-value="parsed"
      :choices="choices"
      :disabled="disabled"
      :secret="secret"
      @update:model-value="update"
    />
    <UiButton v-if="secret && replacing" class="self-start" :disabled="disabled" @click="cancel">{{
      $t('plugins.manager.cancelReplacement')
    }}</UiButton>
  </div>
</template>
<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import UiButton from '../../../components/UiButton.vue'
import PluginJsonField from './PluginJsonField.vue'
import { initialJsonValue, hasEditableJsonShape } from './jsonEditor'
import type { JsonEditorSchema, PluginSettingsChoices } from './types'
const props = defineProps<{
  modelValue: string
  schema: JsonEditorSchema
  choices: PluginSettingsChoices
  disabled?: boolean
  secret?: boolean
}>()
const emit = defineEmits<{ 'update:modelValue': [value: string] }>()
const { t: $t } = useI18n()
const replacing = ref(false)
const draft = ref<unknown>(initialJsonValue(props.schema))
const decoded = computed(() => {
  if (!props.modelValue) return { value: initialJsonValue(props.schema), invalid: false }
  try {
    const value: unknown = JSON.parse(props.modelValue)
    return { value, invalid: !hasEditableJsonShape(value, props.schema) }
  } catch {
    return { value: undefined, invalid: true }
  }
})
const invalid = computed(() => !props.secret && decoded.value.invalid)
const parsed = computed(() => (props.secret ? draft.value : decoded.value.value))
function begin() {
  draft.value = initialJsonValue(props.schema)
  replacing.value = true
}
function cancel() {
  replacing.value = false
  draft.value = initialJsonValue(props.schema)
  emit('update:modelValue', '')
}
function update(value: unknown) {
  draft.value = value
  emit('update:modelValue', JSON.stringify(value))
}
// Save/reload clears transient credentials. No stored secret is ever read into this tree.
watch(
  () => props.modelValue,
  (value, previous) => {
    if (props.secret && !value && previous) {
      replacing.value = false
      draft.value = initialJsonValue(props.schema)
    }
  }
)
</script>
