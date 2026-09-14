<template>
  <div v-if="plugins.length" class="flex flex-col gap-1.5">
    <section v-for="plugin in plugins" :key="plugin.plugin_id" class="classic-card">
      <div class="classic-header break-words">{{ plugin.plugin_id }}</div>
      <output v-if="!canAct(plugin)" class="block px-2 py-1 text-[10px] text-muted">
        {{ $t('plugins.unavailable') }}
      </output>
      <div class="p-1 flex flex-col gap-1">
        <div
          v-for="item in plugin.contributions"
          :key="item.id"
          class="px-1 py-0.5 min-w-0 break-words"
        >
          <p class="text-[10px] font-medium text-muted">{{ item.title }}</p>
          <p v-if="item.kind === 'text'" class="text-[11px] text-main whitespace-pre-wrap">
            {{ item.text }}
          </p>
          <p v-else-if="item.kind === 'metric'" class="text-[11px] font-semibold text-main tabular">
            {{ item.value }}{{ item.unit }}
          </p>
          <output
            v-else-if="item.kind === 'status'"
            class="text-[11px] font-semibold"
            :class="statusClass[item.tone]"
          >
            {{ item.value }}
          </output>
          <template v-else-if="item.kind === 'action'">
            <UiButton
              size="sm"
              :disabled="!canAct(plugin)"
              :loading="pendingActions.has(actionKey(plugin.plugin_id, item.action_id))"
              @click="runAction(plugin.plugin_id, item)"
            >
              {{ item.label }}
            </UiButton>
            <p
              v-if="failedActions.has(actionKey(plugin.plugin_id, item.action_id))"
              role="alert"
              class="text-[10px] text-consumption mt-0.5"
            >
              {{ $t('plugins.actionFailed') }}
            </p>
          </template>
        </div>
      </div>
    </section>
  </div>
</template>

<script setup lang="ts">
import { onMounted, onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'
import UiButton from '../../../components/UiButton.vue'
import { createPluginDashboard } from './usePluginDashboard'

const { t: $t } = useI18n()
const dashboard = createPluginDashboard()
const { plugins, pendingActions, failedActions, canAct, actionKey, runAction } = dashboard
const statusClass = {
  neutral: 'text-main',
  success: 'text-battery',
  warning: 'text-solar',
  error: 'text-consumption',
}
onMounted(() => void dashboard.start())
onUnmounted(dashboard.stop)
</script>
