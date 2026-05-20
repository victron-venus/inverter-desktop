<template>
  <div>
    <v-divider class="mb-4 mt-4">
      <v-chip size="small" color="primary">Home Buttons</v-chip>
    </v-divider>

    <div
      v-if="haEntitiesList.length === 0"
      class="text-caption grey--text mb-2"
    >
      No home buttons configured. Add entities below.
    </div>

    <div
      v-for="(entity, index) in haEntitiesList"
      :key="entity.id || `home-${index}`"
      class="entity-card mb-2 pa-2 rounded border"
    >
      <v-row dense>
        <v-col cols="12" sm="3">
          <v-text-field
            v-model="entity.label"
            label="Label"
            variant="outlined"
            density="compact"
            hide-details
            required
          ></v-text-field>
        </v-col>
        <v-col cols="12" sm="4">
          <v-autocomplete
            v-model="entity.entity"
            :items="discoveredEntities"
            item-title="entity_id"
            item-value="entity_id"
            label="Entity ID"
            variant="outlined"
            density="compact"
            hide-details
            clearable
            :rules="entityRules"
          ></v-autocomplete>
        </v-col>
        <v-col cols="12" sm="2" class="d-flex align-center">
          <v-checkbox
            v-model="entity.enabled"
            label="Active"
            hide-details
            density="compact"
          ></v-checkbox>
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
            :disabled="index === haEntitiesList.length - 1"
          >
            <v-icon>mdi-arrow-down</v-icon>
          </v-btn>
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
      Add Home Entity
    </v-btn>
  </div>
</template>

<script setup lang="ts">
defineProps<{
  haEntitiesList: Array<{ id: string; label: string; entity: string; domain: string; enabled: boolean }>
  discoveredEntities: Array<{ entity_id: string; friendly_name: string; domain: string }>
  entityRules: ((v: string) => boolean | string)[]
}>()

defineEmits<{
  add: []
  remove: [index: number]
  'move-up': [index: number]
  'move-down': [index: number]
}>()
</script>
