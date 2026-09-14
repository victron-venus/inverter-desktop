<template>
  <section class="classic-card p-3 flex flex-col gap-3" :aria-busy="busy || undefined">
    <h3 class="classic-subsection-title">{{ $t('plugins.manager.storedData') }}</h3>
    <p class="text-[11px] text-muted">{{ $t('plugins.manager.storedDataHelp') }}</p>
    <output v-if="busy" class="text-[12px] text-muted">{{ $t('plugins.manager.working') }}</output>
    <p v-if="error" role="alert" class="text-[12px] text-consumption break-words">{{ error }}</p>
    <template v-if="snapshot">
      <p class="text-[12px] text-muted">
        {{
          $t('plugins.manager.storedDataUsage', {
            used: number(snapshot.total_bytes),
            limit: number(snapshot.max_bytes),
            records: number(snapshot.max_records),
          })
        }}
      </p>
      <p class="text-[11px] text-muted">{{ $t('plugins.manager.storedDataPending') }}</p>
      <p v-if="!snapshot.records.length" class="text-[12px] text-muted">
        {{ $t('plugins.manager.noStoredData') }}
      </p>
      <article
        v-for="record in snapshot.records"
        :key="record.record_id"
        class="classic-inset p-2 flex flex-col gap-2"
      >
        <h4 class="font-semibold text-[12px] break-words">
          {{ record.plugin_id ?? $t('plugins.manager.unidentifiedData') }}
        </h4>
        <p class="text-[11px] text-muted break-words">
          <span :title="record.record_id">{{ record.record_id.slice(0, 12) }}…</span>
          · {{ $t('plugins.manager.dataBytes', { bytes: number(record.bytes) }) }}
        </p>
        <p v-if="record.plugin_id !== null" class="text-[11px] text-muted">
          {{ $t('plugins.manager.installedDataProtected') }}
        </p>
        <UiButton
          class="self-start"
          :disabled="disabled || busy || record.plugin_id !== null"
          @click="requestDeletion(record.record_id)"
          >{{ $t('plugins.manager.deleteStoredData') }}</UiButton
        >
        <div v-if="confirmation?.record_id === record.record_id" class="flex flex-col gap-2">
          <p class="text-[12px] break-words">
            {{
              $t('plugins.manager.confirmDeleteData', {
                record: confirmation.record_id,
                bytes: number(confirmation.bytes),
              })
            }}
          </p>
          <div class="flex flex-wrap gap-2">
            <UiButton variant="danger" :disabled="disabled || busy" @click="remove">
              {{ $t('plugins.manager.deleteDataPermanently') }}
            </UiButton>
            <UiButton :disabled="disabled || busy" @click="cancelDeletion">{{
              $t('plugins.manager.cancel')
            }}</UiButton>
          </div>
        </div>
      </article>
    </template>
    <div class="flex flex-wrap gap-2">
      <UiButton :disabled="disabled || busy" @click="refresh">{{
        $t('plugins.manager.refresh')
      }}</UiButton>
      <UiButton :disabled="disabled || busy" @click="close">{{
        $t('plugins.manager.closeStoredData')
      }}</UiButton>
    </div>
  </section>
</template>

<script setup lang="ts">
import { onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'
import UiButton from '../../../components/UiButton.vue'
import type { RetainedPluginDataController } from './useRetainedPluginData'

const props = defineProps<{ controller: RetainedPluginDataController; disabled: boolean }>()
const { t: $t, locale } = useI18n()
const {
  snapshot,
  confirmation,
  busy,
  error,
  close,
  refresh,
  cancelDeletion,
  requestDeletion,
  remove,
} = props.controller
function number(value: number) {
  return new Intl.NumberFormat(locale.value).format(value)
}
onUnmounted(close)
</script>
