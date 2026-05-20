<template>
  <div>
    <v-divider class="mb-4 mt-4">
      <v-chip size="small" color="primary">Header Toggles</v-chip>
    </v-divider>

    <div
      v-for="(toggle, index) in headerTogglesList"
      :key="toggle.id || `toggle-${index}`"
      class="toggle-card mb-2 pa-2 rounded border"
    >
      <v-row dense>
        <v-col cols="12" sm="4">
          <v-text-field
            v-model="toggle.label"
            label="Label"
            variant="outlined"
            density="compact"
            hide-details
            required
          ></v-text-field>
        </v-col>
        <v-col cols="12" sm="5">
          <v-autocomplete
            v-model="toggle.entity"
            :items="discoveredEntityIds"
            label="Entity ID"
            variant="outlined"
            density="compact"
            hide-details
            clearable
            :rules="entityRules"
          ></v-autocomplete>
        </v-col>
        <v-col cols="12" sm="1" class="d-flex align-center justify-center">
          <v-btn
            icon
            size="x-small"
            color="red"
            @click="$emit('remove', index)"
          >
            <v-icon>mdi-delete</v-icon>
          </v-btn>
        </v-col>
        <v-col cols="12" sm="2" class="d-flex flex-column align-center">
          <v-btn
            icon
            size="x-small"
            @click="$emit('move-up', index)"
            :disabled="index === 0"
          >
            <v-icon>mdi-arrow-up</v-icon>
          </v-btn>
          <v-btn
            icon
            size="x-small"
            @click="$emit('move-down', index)"
            :disabled="index === headerTogglesList.length - 1"
          >
            <v-icon>mdi-arrow-down</v-icon>
          </v-btn>
        </v-col>
      </v-row>
    </div>

    <v-btn
      color="primary"
      variant="flat"
      size="small"
      @click="$emit('add')"
      class="mt-2"
    >
      <v-icon start>mdi-plus</v-icon>
      Add Header Toggle
    </v-btn>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'

const props = defineProps<{
  headerTogglesList: Array<{ id: string; label: string; entity: string }>
  discoveredEntities: Array<{ entity_id: string; friendly_name: string; domain: string }>
  entityRules: ((v: string) => boolean | string)[]
}>()

const discoveredEntityIds = computed(() => props.discoveredEntities.map(e => e.entity_id))

defineEmits<{
  add: []
  remove: [index: number]
  'move-up': [index: number]
  'move-down': [index: number]
}>()
</script>
