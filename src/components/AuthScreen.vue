<template>
  <div class="fixed inset-0 z-[200] flex items-center justify-center bg-black/70 backdrop-blur-sm">
    <div class="w-[320px] classic-card !rounded-xl shadow-2xl p-6 flex flex-col gap-4">
      <div class="flex flex-col items-center gap-2">
        <Lock :size="32" class="text-accent" />
        <h2 class="text-lg font-bold text-slate-900 dark:text-slate-100">
          Authentication Required
        </h2>
        <p class="text-[11px] text-slate-500 text-center">
          Enter your credentials to access Inverter Desktop
        </p>
      </div>

      <div class="flex flex-col gap-3">
        <div class="flex flex-col gap-1">
          <label for="aus_username" class="text-[10px] font-medium text-slate-500">Username</label>
          <input
            id="aus_username"
            v-model="username"
            type="text"
            placeholder="Username"
            class="classic-input !h-9 !px-3 !text-[13px] w-full"
            @keyup.enter="handleLogin"
          />
        </div>
        <div class="flex flex-col gap-1">
          <label for="aus_password" class="text-[10px] font-medium text-slate-500">Password</label>
          <input
            id="aus_password"
            v-model="password"
            type="password"
            placeholder="Password"
            class="classic-input !h-9 !px-3 !text-[13px] w-full"
            @keyup.enter="handleLogin"
          />
        </div>
      </div>

      <div v-if="error" class="text-[11px] text-red-500 text-center">
        {{ error }}
      </div>

      <div class="flex flex-col gap-2">
        <UiButton
          variant="primary"
          size="lg"
          class="w-full"
          :loading="loading"
          @click="handleLogin"
        >
          Sign In
        </UiButton>

        <UiButton
          v-if="biometricAvailable"
          size="lg"
          class="w-full"
          :disabled="loading"
          @click="handleBiometric"
        >
          <Fingerprint :size="14" />
          Use Biometric
        </UiButton>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { Lock, Fingerprint } from '@lucide/vue'
import UiButton from './UiButton.vue'
import { logger } from '../logger'

const emit = defineEmits<{
  authenticated: [token: string]
}>()

const username = ref('')
const password = ref('')
const error = ref('')
const loading = ref(false)
const biometricAvailable = ref(false)

onMounted(async () => {
  try {
    biometricAvailable.value = await invoke<boolean>('auth_biometric_available')
  } catch {
    biometricAvailable.value = false
  }
})

async function handleLogin() {
  if (!username.value || !password.value) {
    error.value = 'Please enter username and password'
    return
  }
  loading.value = true
  error.value = ''
  try {
    const token = await invoke<string>('auth_login', {
      username: username.value,
      password: password.value,
    })
    if (token === 'disabled') {
      error.value = 'Authentication is not enabled'
    } else {
      emit('authenticated', token)
    }
  } catch (e) {
    error.value = String(e)
    logger.error('Auth failed:', e)
  } finally {
    loading.value = false
  }
}

async function handleBiometric() {
  loading.value = true
  error.value = ''
  try {
    const token = await invoke<string>('auth_biometric')
    emit('authenticated', token)
  } catch (e) {
    error.value = String(e)
    logger.error('Biometric auth failed:', e)
  } finally {
    loading.value = false
  }
}
</script>
