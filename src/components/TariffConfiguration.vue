<template>
  <section aria-label="Electricity tariff" class="flex flex-col gap-2 text-[12px]">
    <h2 class="classic-section-title">Electricity tariff</h2>
    <p v-if="localMode">
      This local tariff is used only on this device for the current installation. It does not change
      the shared controller tariff. This choice is saved immediately.
    </p>
    <p v-else>
      The controller stores energy prices, seasons and billing dates for every connected dashboard.
      Saving here takes effect immediately on the controller.
    </p>
    <p v-if="plan">{{ plan.name }} · {{ plan.currency }} · {{ plan.timeZone }}</p>
    <p v-else>No {{ localMode ? 'local' : 'controller' }} tariff is configured.</p>
    <p v-if="!localMode && !writable">
      Connect to an updated inverter-control to edit the shared tariff.
    </p>
    <p v-if="error" role="alert">{{ error }}</p>
    <div class="flex flex-wrap gap-2">
      <button type="button" class="classic-input" :disabled="!editable" @click="openEditor">
        {{ plan ? 'Edit' : 'Set' }} {{ localMode ? 'local' : 'controller' }} tariff
      </button>
      <button
        type="button"
        class="classic-input"
        :disabled="!scope || !local.ready.value || changingMode"
        @click="toggleMode"
      >
        {{ localMode ? 'Use controller tariff' : 'Use a local tariff on this device' }}
      </button>
      <button
        v-if="plan"
        type="button"
        class="classic-input"
        :disabled="!scope"
        @click="intervalsOpen = true"
      >
        Interval energy cost
      </button>
    </div>
    <TariffEditor
      v-if="editor"
      :plan="editor.plan"
      :tariff-scope="editor.scope"
      :persist="false"
      :destination="editor.mode"
      :save-plan="editor.save"
      :clear-label="editor.mode === 'local' ? 'Clear local tariff' : 'Clear controller tariff'"
      @saved="editor = null"
      @close="editor = null"
    />
    <IntervalEnergy
      v-if="plan && scope && intervalsOpen"
      :plan="plan"
      :tariff-scope="scope"
      @close="intervalsOpen = false"
    />
  </section>
</template>
<script setup lang="ts">
import {
  computed,
  defineAsyncComponent,
  onMounted,
  onBeforeUnmount,
  ref,
  shallowRef,
  watch,
} from 'vue'
import { isTauri } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getAppConfig } from '../config'
import { validatePlan, type TariffPlan } from '../tariffs/model'
import { tariffScope } from '../tariffs/scope'
import { saveLocalPreference, type TariffMode } from '../tariffs/localPreference'
import { useLocalTariff } from '../tariffs/useLocalTariff'
import {
  readControllerTariff,
  saveControllerTariff,
  type ControllerTariff,
} from '../tariffs/controller'

const TariffEditor = defineAsyncComponent(() => import('../tariffs/TariffEditor.vue'))
const IntervalEnergy = defineAsyncComponent(() => import('../tariffs/IntervalEnergy.vue'))
const controller = ref<ControllerTariff>({ plan: null })
const scope = ref<string | null>(null)
const local = useLocalTariff(() => scope.value)
const localMode = computed(() => local.mode.value === 'local')
const editor = shallowRef<{
  plan: TariffPlan | null
  mode: TariffMode
  scope: string
  save: (value: TariffPlan | null) => Promise<void>
} | null>(null)
const intervalsOpen = ref(false)
const changingMode = ref(false)
const controllerError = ref('')
const scopeError = ref('')
const actionError = ref('')
let mounted = true
let generation = 0
let timer: ReturnType<typeof setTimeout> | undefined
let unlisten: UnlistenFn | undefined
const plan = computed(() => {
  if (localMode.value) return local.plan.value
  try {
    return controller.value.plan == null ? null : validatePlan(controller.value.plan)
  } catch {
    return null
  }
})
const error = computed(() =>
  [
    scopeError.value,
    actionError.value,
    local.syncError.value,
    localMode.value ? local.error.value : controllerError.value,
  ]
    .filter(Boolean)
    .join(' ')
)
const writable = computed(
  () => controller.value.status?.writable === true && !!controller.value.status?.revision
)
const editable = computed(
  () => !!scope.value && (localMode.value ? local.ready.value : writable.value)
)
watch([scope, localMode], () => {
  editor.value = null
  intervalsOpen.value = false
  actionError.value = ''
})
async function refresh(epoch = generation) {
  try {
    const value = await readControllerTariff()
    if (!mounted || epoch !== generation) return
    if (value.plan != null) validatePlan(value.plan)
    controller.value = value
    controllerError.value = ''
  } catch (cause) {
    if (mounted && epoch === generation) {
      controllerError.value = String(cause)
      controller.value = { plan: null }
    }
  }
}
async function loadScope() {
  const epoch = ++generation
  scope.value = null
  controller.value = { plan: null }
  scopeError.value = ''
  try {
    // Use the saved connection, never the Configuration form's unsaved fields.
    const config = await getAppConfig()
    if (!mounted || epoch !== generation) return
    scope.value = tariffScope(config)
    await refresh(epoch)
  } catch {
    if (mounted && epoch === generation)
      scopeError.value = 'The current installation could not be loaded.'
  }
}
async function poll() {
  if (scope.value && !editor.value) await refresh()
  if (mounted) timer = setTimeout(poll, 2000)
}
function openEditor() {
  const targetScope = scope.value
  if (!targetScope || !editable.value) return
  const mode = local.mode.value
  const revision = controller.value.status?.revision ?? ''
  editor.value = {
    plan: plan.value,
    mode,
    scope: targetScope,
    save: async (value) => {
      if (!mounted || scope.value !== targetScope)
        throw new Error('The installation changed. Reopen the tariff editor before saving.')
      if (mode === 'local') {
        await saveLocalPreference({ scope: targetScope, kind: 'plan', value })
      } else {
        await saveControllerTariff(value, revision)
        await refresh()
      }
    },
  }
}
async function toggleMode() {
  const targetScope = scope.value
  if (!targetScope || !local.ready.value || changingMode.value) return
  changingMode.value = true
  actionError.value = ''
  try {
    await saveLocalPreference({
      scope: targetScope,
      kind: 'mode',
      value: localMode.value ? 'controller' : 'local',
    })
  } catch (cause) {
    if (mounted && scope.value === targetScope) actionError.value = String(cause)
  } finally {
    changingMode.value = false
  }
}
onMounted(async () => {
  if (isTauri()) {
    try {
      const stop = await listen('config-saved', loadScope)
      if (!mounted) return stop()
      unlisten = stop
    } catch {
      scopeError.value = 'Configuration synchronization is unavailable. Reopen this window.'
      return
    }
  }
  await loadScope()
  if (mounted) timer = setTimeout(poll, 2000)
})
onBeforeUnmount(() => {
  mounted = false
  unlisten?.()
  clearTimeout(timer)
})
</script>
