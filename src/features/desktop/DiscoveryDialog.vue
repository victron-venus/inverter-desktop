<template>
  <!-- Discovery Dialog (Custom) -->
  <div
    v-if="discoveryDialog"
    class="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/30 backdrop-blur-[2px]"
  >
    <div
      class="classic-card w-full max-w-sm max-h-[80vh] flex flex-col overflow-hidden dark:bg-[#121212] shadow-2xl animate-in fade-in duration-150"
    >
      <header
        class="p-3 border-b border-black/[0.06] dark:border-white/[0.07] flex items-center justify-between bg-[#f6f6f8] dark:bg-[#121214]"
      >
        <h3 class="classic-subsection-title text-xs">Discover Entities</h3>
        <button
          type="button"
          @click="discoveryDialog = false"
          class="text-slate-900 dark:text-slate-300 hover:text-slate-900 dark:hover:text-slate-200"
        >
          <X :size="16" />
        </button>
      </header>
      <div class="flex-1 overflow-y-auto p-2 flex flex-col gap-1">
        <div v-if="discoveryLoading" class="flex flex-col items-center justify-center py-10 gap-2">
          <Loader2 class="animate-spin text-accent" :size="20" />
          <span class="classic-label text-slate-900 dark:text-slate-300">Fetching...</span>
        </div>
        <template v-else>
          <div class="p-1 sticky top-0 bg-[#121212] dark:bg-[#121212]">
            <label for="discovery_search" class="sr-only">Search entities</label>
            <input
              id="discovery_search"
              v-model="discoverySearch"
              type="text"
              placeholder="Search entities..."
              class="classic-input w-full"
            />
          </div>
          <div
            v-if="filteredDiscoveredEntities.length === 0"
            class="classic-label text-center py-8"
          >
            No matches
          </div>
          <button
            type="button"
            v-for="e in filteredDiscoveredEntities"
            :key="e.entity_id"
            @click="toggleSelection(e.entity_id)"
            class="w-full text-left p-2 rounded border border-transparent cursor-pointer transition-all flex items-center justify-between group"
            :class="
              selectedDiscovery.includes(e.entity_id)
                ? 'bg-accent/10 border-accent/20'
                : 'hover:bg-slate-50 dark:hover:bg-slate-800'
            "
          >
            <span class="block">
              <span
                class="block text-[11px] font-bold group-hover:text-accent transition-colors"
                :class="{
                  'text-accent': selectedDiscovery.includes(e.entity_id),
                  'dark:text-slate-300': !selectedDiscovery.includes(e.entity_id),
                }"
              >
                {{ e.friendly_name }}
              </span>
              <span class="block text-[9px] text-muted font-mono">
                {{ e.entity_id }}
              </span>
            </span>
            <span v-if="selectedDiscovery.includes(e.entity_id)" class="block text-accent">
              <Check :size="12" />
            </span>
          </button>
        </template>
      </div>
      <footer
        class="p-3 border-t border-black/[0.06] dark:border-white/[0.07] flex flex-col gap-2 bg-[#f6f6f8] dark:bg-[#121214]"
      >
        <div class="flex gap-1 p-0.5 bg-slate-200/50 dark:bg-slate-800 rounded">
          <button
            type="button"
            @click="discoveryTargetGroup = 'home'"
            class="flex-1 py-1 rounded-md text-[10px] font-semibold transition-all tracking-tight"
            :class="
              discoveryTargetGroup === 'home'
                ? 'bg-white dark:bg-slate-700 shadow-sm dark:text-white'
                : 'text-slate-500 opacity-50 dark:text-slate-400'
            "
          >
            Home Buttons
          </button>
          <button
            type="button"
            @click="discoveryTargetGroup = 'toggle'"
            class="flex-1 py-1 rounded-md text-[10px] font-semibold transition-all tracking-tight"
            :class="
              discoveryTargetGroup === 'toggle'
                ? 'bg-white dark:bg-slate-700 shadow-sm dark:text-white'
                : 'text-slate-500 opacity-50 dark:text-slate-400'
            "
          >
            {{ $t('config.headerControlsTitle') }}
          </button>
        </div>
        <div class="flex gap-2">
          <UiButton class="flex-1" @click="discoveryDialog = false"> Cancel </UiButton>
          <UiButton
            variant="primary"
            class="flex-1"
            :disabled="!selectedDiscovery.length"
            @click="addDiscoveredEntities"
          >
            Add ({{ selectedDiscovery.length }})
          </UiButton>
        </div>
      </footer>
    </div>
  </div>
</template>
<script setup lang="ts">
import type { useDashboardControlsConfig } from '../../composables/useDashboardControlsConfig'
import { Check, Loader2, X } from '@lucide/vue'
import UiButton from '../../components/UiButton.vue'
const { controls } = defineProps<{ controls: ReturnType<typeof useDashboardControlsConfig> }>()
const {
  discoveryDialog,
  discoveryLoading,
  discoverySearch,
  filteredDiscoveredEntities,
  selectedDiscovery,
  discoveryTargetGroup,
  addDiscoveredEntities,
} = controls
const toggleSelection = (id: string) => {
  const index = selectedDiscovery.value.indexOf(id)
  if (index > -1) selectedDiscovery.value.splice(index, 1)
  else selectedDiscovery.value.push(id)
}
</script>
