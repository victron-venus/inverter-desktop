<template>
  <span class="inline-flex flex-col items-start gap-1">
    <a
      :href="PRIVACY_POLICY_URL"
      class="text-accent underline underline-offset-2"
      @click.prevent="openPolicy"
      >Privacy policy</a
    >
    <span v-if="error" role="alert" class="text-consumption">{{ error }}</span>
  </span>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import { openUrl } from '@tauri-apps/plugin-opener'

const PRIVACY_POLICY_URL =
  'https://github.com/victron-venus/inverter-desktop/blob/main/docs/privacy-policy.md'
const error = ref('')

async function openPolicy() {
  error.value = ''
  try {
    await openUrl(PRIVACY_POLICY_URL)
  } catch {
    error.value = 'Could not open the privacy policy. Try again when a browser is available.'
  }
}
</script>
