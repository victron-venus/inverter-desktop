<template>
  <div class="fixed inset-0 z-[200] flex items-center justify-center bg-black/70 backdrop-blur-sm">
    <div
      class="w-[320px] bg-white dark:bg-[#1a1a1a] rounded-lg shadow-2xl border border-slate-200 dark:border-slate-700 p-6 flex flex-col gap-4"
    >
      <div class="flex flex-col items-center gap-2">
        <Lock :size="32" class="text-accent" />
        <h2 class="text-lg font-bold text-slate-900 dark:text-slate-100">
          Authentication Required
        </h2>
        <p class="text-[11px] text-slate-500 text-center">
          Enter your credentials to access Inverter Dashboard
        </p>
      </div>

      <div class="flex flex-col gap-3">
        <div class="flex flex-col gap-1">
          <label class="text-[10px] font-medium text-slate-500">Username</label>
          <input
            v-model="username"
            type="text"
            placeholder="Username"
            class="rounded border border-slate-300 dark:border-slate-600 bg-white dark:bg-[#0a0a0a] px-3 py-2 text-[13px] text-slate-700 dark:text-slate-300 focus:outline-none focus:ring-2 focus:ring-accent"
            @keyup.enter="handleLogin"
          />
        </div>
        <div class="flex flex-col gap-1">
          <label class="text-[10px] font-medium text-slate-500">Password</label>
          <input
            v-model="password"
            type="password"
            placeholder="Password"
            class="rounded border border-slate-300 dark:border-slate-600 bg-white dark:bg-[#0a0a0a] px-3 py-2 text-[13px] text-slate-700 dark:text-slate-300 focus:outline-none focus:ring-2 focus:ring-accent"
            @keyup.enter="handleLogin"
          />
        </div>
      </div>

      <div v-if="error" class="text-[11px] text-red-500 text-center">
        {{ error }}
      </div>

      <div class="flex flex-col gap-2">
        <button
          @click="handleLogin"
          :disabled="loading"
          class="classic-btn !h-[36px] !bg-accent !border-emerald-600 !text-white flex items-center justify-center gap-2"
        >
          <Loader2 v-if="loading" :size="14" class="animate-spin" />
          <span>Sign In</span>
        </button>

        <button
          v-if="biometricAvailable"
          @click="handleBiometric"
          :disabled="loading"
          class="classic-btn !h-[36px] flex items-center justify-center gap-2"
        >
          <Fingerprint :size="14" />
          <span>Use Biometric</span>
        </button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { Lock, Loader2, Fingerprint } from 'lucide-vue-next'
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
