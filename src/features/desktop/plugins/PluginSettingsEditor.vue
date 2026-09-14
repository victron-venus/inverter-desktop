<template>
  <form
    class="classic-inset p-3 flex flex-col gap-3"
    :aria-busy="busy || undefined"
    @submit.prevent="save"
    @input="saved = false"
    @change="saved = false"
  >
    <h4 class="classic-subsection-title">{{ $t('plugins.manager.settings') }}</h4>
    <p class="text-[11px] text-muted">{{ $t('plugins.manager.settingsHelp') }}</p>
    <output v-if="busy" class="text-[12px] text-muted">{{ $t('plugins.manager.working') }}</output>
    <p v-if="error" role="alert" class="text-[12px] text-consumption break-words">{{ error }}</p>
    <template v-if="saved">
      <output class="text-[12px] text-muted">{{ $t('plugins.manager.settingsSaved') }}</output>
      <p v-if="restartError" role="alert" class="text-[12px] text-consumption break-words">
        {{ $t('plugins.manager.settingsRestartFailed') }} {{ restartError }}
      </p>
    </template>

    <template v-if="settings">
      <p v-if="!settings.fields.length" class="text-[12px] text-muted">
        {{ $t('plugins.manager.noSettings') }}
      </p>
      <div v-for="field in settings.fields" :key="field.key" class="flex flex-col gap-1 min-w-0">
        <label :for="fieldId(field.key)" class="classic-label break-words">
          {{ field.title }}{{ field.required ? ' *' : '' }}
        </label>
        <p
          v-if="field.description"
          :id="`${fieldId(field.key)}-help`"
          class="text-[11px] text-muted break-words"
        >
          {{ field.description }}
        </p>
        <template v-if="field.secret">
          <input
            :id="fieldId(field.key)"
            :name="field.key"
            type="password"
            class="classic-input"
            autocomplete="new-password"
            :value="secretChanges[field.key] ?? ''"
            :disabled="busy || secretChanges[field.key] === null"
            :required="field.required && !settings.secret_present[field.key]"
            maxlength="4096"
            :aria-describedby="descriptionId(field)"
            @input="setSecret(field.key, ($event.target as HTMLInputElement).value)"
          />
          <output class="text-[11px] text-muted">
            {{
              $t(
                settings.secret_present[field.key]
                  ? 'plugins.manager.secretStored'
                  : 'plugins.manager.secretNotStored'
              )
            }}
          </output>
          <p class="text-[11px] text-muted">{{ $t('plugins.manager.secretHelp') }}</p>
          <label
            v-if="settings.secret_present[field.key]"
            class="flex items-center gap-2 text-[11px]"
          >
            <input
              type="checkbox"
              :name="`${field.key}-remove`"
              :checked="secretChanges[field.key] === null"
              :disabled="busy"
              @change="removeSecret(field.key, ($event.target as HTMLInputElement).checked)"
            />
            {{ $t('plugins.manager.removeSecret') }}
          </label>
        </template>
        <select
          v-else-if="field.enum"
          :id="fieldId(field.key)"
          v-model="values[field.key]"
          :name="field.key"
          class="classic-input"
          :disabled="busy"
          :required="field.required"
          :aria-describedby="descriptionId(field)"
        >
          <option value="">{{ $t('plugins.manager.chooseValue') }}</option>
          <option v-for="option in field.enum" :key="option" :value="option">{{ option }}</option>
        </select>
        <input
          v-else-if="field.type === 'boolean'"
          :id="fieldId(field.key)"
          :name="field.key"
          type="checkbox"
          class="self-start"
          :checked="values[field.key] === true"
          :disabled="busy"
          :aria-describedby="descriptionId(field)"
          @change="values[field.key] = ($event.target as HTMLInputElement).checked"
        />
        <input
          v-else
          :id="fieldId(field.key)"
          :value="values[field.key] ?? ''"
          :name="field.key"
          :type="field.type === 'string' ? 'text' : 'number'"
          class="classic-input"
          :step="field.type === 'integer' ? 1 : 'any'"
          :min="field.minimum ?? undefined"
          :max="field.maximum ?? undefined"
          maxlength="4096"
          :disabled="busy"
          :required="field.required"
          :aria-describedby="descriptionId(field)"
          @input="values[field.key] = ($event.target as HTMLInputElement).value"
        />
      </div>
    </template>

    <div class="flex flex-wrap gap-2">
      <UiButton
        type="submit"
        variant="primary"
        :disabled="busy || !settings || !settings.fields.length"
      >
        {{ $t('plugins.manager.saveSettings') }}
      </UiButton>
      <UiButton :disabled="busy" @click="load">{{ $t('plugins.manager.reloadSettings') }}</UiButton>
      <UiButton :disabled="busy" @click="close">{{ $t('plugins.manager.cancel') }}</UiButton>
    </div>
  </form>
</template>

<script setup lang="ts">
import { onMounted, onUnmounted, useId } from 'vue'
import { useI18n } from 'vue-i18n'
import UiButton from '../../../components/UiButton.vue'
import type { PluginSettingsField } from './types'
import { createPluginSettings } from './usePluginSettings'

const props = defineProps<{ pluginId: string; version: string; editorKey: number }>()
const emit = defineEmits<{
  busy: [key: number, value: boolean]
  close: [key: number]
  saved: [key: number]
}>()
const { t: $t } = useI18n()
const prefix = useId()
const editor = createPluginSettings(
  props.pluginId,
  props.version,
  () => $t('plugins.manager.settingsFailed'),
  (value) => emit('busy', props.editorKey, value),
  (field) => $t('plugins.manager.invalidSetting', { field })
)
const {
  settings,
  values,
  secretChanges,
  busy,
  error,
  saved,
  restartError,
  load,
  setSecret,
  removeSecret,
} = editor
function fieldId(key: string) {
  return `${prefix}-${key}`
}
function descriptionId(field: PluginSettingsField) {
  return field.description ? `${fieldId(field.key)}-help` : undefined
}
async function save() {
  if (await editor.save()) emit('saved', props.editorKey)
}
function close() {
  editor.stop()
  emit('close', props.editorKey)
}
onMounted(() => void load())
onUnmounted(editor.stop)
</script>
