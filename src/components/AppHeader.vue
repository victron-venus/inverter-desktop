<template>
  <div class="card mb-2">
    <div class="card-body py-1 px-2">
      <div class="d-flex flex-wrap gap-1 align-items-center">
        <v-btn
          outlined
          class="custom-3d"
          :class="dryRun ? 'state-on' : 'state-off'"
          size="small"
          @click="$emit('send', 'dry_run')"
        >
          <i class="fas fa-flask me-1"></i>DRY
        </v-btn>
        <v-btn
          outlined
          class="custom-3d"
          :class="essClass === 'on' ? 'state-on' : 'state-off'"
          size="small"
          @click="$emit('send', 'ess_mode')"
        >
          <i class="fas fa-bolt me-1"></i>{{ essText }}
        </v-btn>
        <div class="vr mx-1" style="border-left:1px solid #ccc;height:16px;"></div>
        <v-btn
          v-for="toggle in headerToggles"
          :key="toggle.id"
          outlined
          class="custom-3d"
          :class="booleans?.[toggle.id] === true ? 'state-on' : 'state-off'"
          size="small"
          @click="$emit('send', 'toggle', {entity: toggle.entity})"
        >
          {{ toggle.label }}
        </v-btn>
        <div class="ms-auto d-flex gap-1">
          <v-btn
            outlined
            class="custom-3d"
            icon
            size="small"
            @click="$emit('toggle-theme')"
            id="theme-btn"
          >
            <i class="fas" :class="isDark ? 'fa-sun' : 'fa-moon'"></i>
          </v-btn>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
defineProps<{
  dryRun: boolean
  essClass: string
  essText: string
  headerToggles: Array<{ id: string; label: string; entity: string }>
  booleans: Record<string, boolean> | undefined
  isDark: boolean
}>()
defineEmits<{
  send: [action: string, payload?: any]
  'toggle-theme': []
}>()
</script>
