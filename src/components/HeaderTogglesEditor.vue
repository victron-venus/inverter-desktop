<template>
  <div class="flex flex-col gap-2">
    <div class="flex items-center justify-between px-1">
      <h3 class="classic-subsection-title">{{ $t('config.headerControlsTitle') }}</h3>
      <button
        type="button"
        @click="$emit('add')"
        class="text-[10px] font-semibold text-accent hover:opacity-80 flex items-center gap-1"
      >
        <Plus :size="12" /> {{ $t('config.addHeaderControl') }}
      </button>
    </div>

    <p class="text-[10px] text-muted px-1">{{ $t('config.headerControlsHelp') }}</p>

    <div class="flex flex-wrap gap-1" :aria-label="$t('config.inverterControlPresets')">
      <button
        v-for="control in DEFAULT_INVERTER_CONTROLS"
        :key="control.entity"
        type="button"
        :disabled="hasInverterControl(control.entity)"
        :data-control="control.entity"
        @click="$emit('add', control)"
        class="px-2 py-1 text-[10px] rounded-md border border-black/10 dark:border-white/10 text-accent disabled:opacity-40 disabled:cursor-default"
      >
        {{ control.label }}
      </button>
    </div>

    <div
      v-if="headerTogglesList.length === 0"
      class="py-4 text-center border border-dashed border-black/10 dark:border-white/10 rounded-lg text-[11px] text-muted bg-black/[0.015] dark:bg-white/[0.02]"
    >
      {{ $t('config.noHeaderControls') }}
    </div>

    <div class="flex flex-col gap-1.5">
      <div
        v-for="(toggle, index) in headerTogglesList"
        :key="toggle.id || `toggle-${index}`"
        class="classic-inset !rounded-lg p-2 flex items-center gap-2"
      >
        <div class="flex-1 grid grid-cols-2 gap-2">
          <div class="flex flex-col gap-0.5">
            <label :for="'ht-label-' + index" class="classic-label px-1">Label</label>
            <input
              :id="'ht-label-' + index"
              v-model="toggle.label"
              type="text"
              class="classic-input !h-7 w-full"
              placeholder="Name"
            />
          </div>
          <div class="flex flex-col gap-0.5">
            <label :for="'ht-entity-' + index" class="classic-label px-1">{{
              $t('config.headerControlTarget')
            }}</label>
            <ControlTargetInput
              :id="'ht-entity-' + index"
              v-model="toggle.entity"
              :entities="controlTargets"
              placeholder="Control target"
              @focus="$emit('focus-entity')"
            />
            <p
              v-if="toggle.entity && !isDashboardControlTarget(toggle.entity)"
              role="alert"
              class="text-[10px] text-consumption px-1"
            >
              {{ $t('config.invalidHeaderControlTarget') }}
            </p>
          </div>
        </div>

        <div class="flex items-center gap-0.5 pt-3">
          <button
            type="button"
            @click="$emit('move-up', index)"
            :disabled="index === 0"
            class="p-1 rounded-md hover:bg-black/[0.04] dark:hover:bg-white/[0.06] disabled:opacity-20 text-muted"
          >
            <ChevronUp :size="14" />
          </button>
          <button
            type="button"
            @click="$emit('move-down', index)"
            :disabled="index === headerTogglesList.length - 1"
            class="p-1 rounded-md hover:bg-black/[0.04] dark:hover:bg-white/[0.06] disabled:opacity-20 text-muted"
          >
            <ChevronDown :size="14" />
          </button>
          <button
            type="button"
            @click="$emit('remove', index)"
            class="p-1 rounded-md hover:bg-red-50 dark:hover:bg-red-950/20 hover:text-consumption transition-colors text-muted/50"
          >
            <Trash2 :size="14" />
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { Plus, Trash2, ChevronUp, ChevronDown } from '@lucide/vue'
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  DEFAULT_INVERTER_CONTROLS,
  inverterControlFlagKey,
  type DashboardControl,
} from '../inverterControl'
import { isDashboardControlTarget } from '../dashboardControlTarget'
import { ControlTargetInput } from '@features'

const props = defineProps<{
  headerTogglesList: DashboardControl[]
  discoveredEntities: Array<{ entity_id: string; friendly_name: string; domain: string }>
}>()

defineEmits<{
  add: [control?: DashboardControl]
  remove: [index: number]
  'move-up': [index: number]
  'move-down': [index: number]
  'focus-entity': []
}>()

const { t: $t } = useI18n()

const controlTargets = computed(() => [
  ...DEFAULT_INVERTER_CONTROLS.map((control) => ({
    entity_id: control.entity,
    friendly_name: control.label,
    domain: 'inverter_control',
  })),
  ...props.discoveredEntities,
])

function hasInverterControl(flag: string): boolean {
  return props.headerTogglesList.some((control) => inverterControlFlagKey(control.entity) === flag)
}
</script>
