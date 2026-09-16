<template>
  <div v-if="sidebar.length" class="flex flex-col gap-1.5">
    <template v-for="{ plugin, item, key } in sidebar" :key="key">
      <section v-if="item.kind === 'weather'" class="classic-card" :data-presentation-id="item.id">
        <div class="classic-header flex items-center gap-1.5">
          <CloudSun :size="10" />{{ item.title }}
        </div>
        <div class="p-1">
          <div class="flex items-center gap-1">
            <span class="text-lg font-bold text-main tabular tracking-tight"
              >{{ item.temperature }}{{ item.unit }}</span
            ><span class="text-[10px] text-muted capitalize">{{ item.condition }}</span>
          </div>
          <div v-if="item.forecast.length" class="mt-1 flex gap-1 overflow-x-auto">
            <div
              v-for="(day, index) in item.forecast.slice(0, 5)"
              :key="index"
              class="classic-inset flex flex-col items-center min-w-[40px] !px-1 !py-0.5"
            >
              <span class="text-[8px] text-muted">{{ day.datetime.slice(5, 10) }}</span
              ><span class="text-[10px] font-bold">{{ day.temperature }}{{ item.unit }}</span
              ><span class="text-[8px] text-muted capitalize truncate max-w-[36px]">{{
                day.condition
              }}</span>
            </div>
          </div>
        </div>
      </section>
      <section
        v-else-if="item.kind === 'summary'"
        class="classic-card px-2 py-1 flex items-center gap-1.5"
        :data-presentation-id="item.id"
      >
        <span class="text-[10px] font-semibold text-muted tracking-tight shrink-0">{{
          item.title
        }}</span>
        <div class="ml-auto flex flex-wrap items-center gap-1.5">
          <PluginCompactAction
            v-for="(action, index) in item.actions"
            :key="`${index}:${action.id}`"
            :plugin="plugin"
            :reference="action"
          />
          <span v-if="item.active" class="text-[10px] font-semibold text-battery">{{
            $t('plugins.running')
          }}</span>
          <span class="text-[11px] font-semibold text-main tabular">{{ item.text }}</span>
        </div>
      </section>
      <section
        v-else-if="item.kind === 'group' && item.rows.length"
        class="classic-card"
        :data-presentation-id="item.id"
      >
        <button
          type="button"
          class="w-full text-left classic-header flex items-center gap-1.5 cursor-pointer hover:opacity-80"
          :aria-expanded="isExpanded(key, item.collapsed)"
          @click="expanded[key] = !isExpanded(key, item.collapsed)"
        >
          <component
            :is="presentationIcon(item.icon)"
            v-if="presentationIcon(item.icon)"
            :size="10"
          />{{ item.title }} ({{ item.rows.length }})<span class="ml-auto text-[10px]">{{
            isExpanded(key, item.collapsed) ? '▾' : '▸'
          }}</span>
        </button>
        <div v-if="isExpanded(key, item.collapsed)" class="p-1 flex flex-col gap-1">
          <div
            v-for="row in item.rows"
            :key="row.id"
            class="flex flex-col gap-0.5"
            :data-presentation-row="row.id"
          >
            <div class="row-hover flex items-center justify-between gap-1 px-1">
              <span class="text-[10px] font-medium text-muted truncate">{{ row.title }}</span>
              <span class="text-[11px] font-semibold text-main tabular">{{
                contributionText(contribution(plugin, row.value))
              }}</span>
              <div v-if="row.actions.length" class="flex gap-0.5 shrink-0">
                <PluginCompactAction
                  v-for="(action, index) in row.actions"
                  :key="`${index}:${action.id}`"
                  :plugin="plugin"
                  :reference="action"
                />
              </div>
            </div>
            <PluginNumberSlider
              v-for="input in numberInputs(plugin, row.input)"
              :key="input.id"
              :input="input"
              :disabled="!dashboard.canAct(plugin)"
              :pending="numberPending(plugin, row.input)"
              :failed="numberFailed(plugin, row.input)"
              @submit="
                (input, value) =>
                  dashboard.runNumberInput(plugin.plugin_id, plugin.instance_id, input, value)
              "
            />
          </div>
        </div>
      </section>
    </template>
    <output v-if="dashboard.unavailable.value" class="text-[10px] text-consumption px-1">{{
      $t('plugins.unavailable')
    }}</output>
  </div>
</template>
<script setup lang="ts">
import { reactive } from 'vue'
import { CloudSun } from '@lucide/vue'
import { useI18n } from 'vue-i18n'
import {
  contribution,
  contributionText,
  numberContribution,
  presentationIcon,
  usePluginPresentation,
} from './presentation'
import type { PluginSnapshot } from './types'
import PluginCompactAction from './PluginCompactAction.vue'
import PluginNumberSlider from './PluginNumberSlider.vue'
const { t: $t } = useI18n()
const { dashboard, sidebar } = usePluginPresentation()
const expanded = reactive<Record<string, boolean>>({})
function isExpanded(key: string, collapsed: boolean) {
  return expanded[key] ?? !collapsed
}
function numberInputs(plugin: PluginSnapshot, id?: string | null) {
  const input = numberContribution(plugin, id)
  return input ? [input] : []
}
function numberPending(plugin: PluginSnapshot, id?: string | null) {
  const input = numberContribution(plugin, id)
  return (
    !!input &&
    dashboard.pendingActions.value.has(
      dashboard.numberInputKey(plugin.plugin_id, plugin.instance_id, input)
    )
  )
}
function numberFailed(plugin: PluginSnapshot, id?: string | null) {
  const input = numberContribution(plugin, id)
  return (
    !!input &&
    dashboard.failedActions.value.has(
      dashboard.numberInputKey(plugin.plugin_id, plugin.instance_id, input)
    )
  )
}
</script>
