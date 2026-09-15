<template>
  <div v-if="panels.length" class="flex flex-col gap-1.5">
    <section
      v-for="{ plugin, cards } in panels"
      :key="JSON.stringify([plugin.plugin_id, plugin.instance_id])"
      class="classic-card"
      :data-plugin-id="plugin.plugin_id"
    >
      <div class="classic-header break-words">{{ plugin.plugin_id }}</div>
      <output v-if="!canAct(plugin)" class="block px-2 py-1 text-[10px] text-muted">
        {{ $t('plugins.unavailable') }}
      </output>
      <div class="p-1 flex flex-col gap-1">
        <div
          v-for="card in cards"
          :key="card.item.id"
          :data-card-id="card.item.id"
          :data-state-card="card.controls.length ? card.item.id : undefined"
          class="px-1 py-0.5 min-w-0 break-words flex flex-wrap items-start gap-1"
          :class="{ 'classic-card p-1.5': card.controls.length }"
        >
          <PluginContribution
            v-for="(item, index) in [card.item, ...card.controls]"
            :key="item.id"
            :item="item"
            :show-title="index === 0"
            :class="{ 'w-full': index === 0 || item.kind === 'number_input' }"
            :disabled="!canAct(plugin)"
            :pending="pendingActions.has(operationKey(plugin, item))"
            :failed="failedActions.has(operationKey(plugin, item))"
            @action="(action) => runAction(plugin.plugin_id, plugin.instance_id, action)"
            @number="
              (input, value) => runNumberInput(plugin.plugin_id, plugin.instance_id, input, value)
            "
          />
        </div>
      </div>
    </section>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { contributionCards } from './contributionCards'
import PluginContribution from './PluginContribution.vue'
import type { DashboardContribution, PluginSnapshot } from './types'
import { createPluginDashboard } from './usePluginDashboard'

const { t: $t } = useI18n()
const dashboard = createPluginDashboard()
const {
  plugins,
  pendingActions,
  failedActions,
  canAct,
  actionKey,
  runAction,
  numberInputKey,
  runNumberInput,
} = dashboard
const panels = computed(() =>
  plugins.value.map((plugin) => ({ plugin, cards: contributionCards(plugin.contributions) }))
)

function operationKey(plugin: PluginSnapshot, item: DashboardContribution): string {
  if (item.kind === 'action') return actionKey(plugin.plugin_id, plugin.instance_id, item)
  if (item.kind === 'number_input')
    return numberInputKey(plugin.plugin_id, plugin.instance_id, item)
  return ''
}

onMounted(() => void dashboard.start())
onUnmounted(dashboard.stop)
</script>
