<template>
  <section class="flex flex-col gap-4" :aria-busy="busy || loading || undefined">
    <header class="border-b border-black/[0.06] dark:border-white/[0.07] pb-2">
      <h2 class="classic-section-title">{{ $t('plugins.manager.title') }}</h2>
      <p class="text-[11px] text-muted mt-1">{{ $t('plugins.manager.intro') }}</p>
      <p class="text-[11px] text-muted mt-1">{{ $t('plugins.manager.immediate') }}</p>
    </header>

    <output v-if="loading" class="text-[12px] text-muted">
      {{ $t('plugins.manager.loading') }}
    </output>
    <output v-if="busy" class="text-[12px] text-muted">
      {{ $t('plugins.manager.working') }}
    </output>
    <div
      v-if="error || snapshot?.error || (!loading && snapshot && !snapshot.ready)"
      class="flex flex-col items-start gap-2"
    >
      <p role="alert" class="text-[12px] text-consumption break-words">
        {{ error || snapshot?.error || $t('plugins.manager.notReady') }}
      </p>
      <p v-if="installFailed" class="text-[11px] text-muted">
        {{ $t('plugins.manager.pickAgain') }}
      </p>
      <UiButton :disabled="busy" @click="retry">{{ $t('plugins.manager.refresh') }}</UiButton>
    </div>

    <template v-if="snapshot">
      <p v-if="snapshot.configuration_error" role="alert" class="text-[12px] text-consumption">
        {{ snapshot.configuration_error }}
      </p>
      <section v-if="snapshot.configured?.length" class="classic-card p-3 flex flex-col gap-2">
        <h3 class="classic-subsection-title">{{ $t('plugins.manager.configuredTitle') }}</h3>
        <p class="text-[11px] text-muted">{{ $t('plugins.manager.configuredHelp') }}</p>
        <div
          v-for="plugin in snapshot.configured"
          :key="plugin.plugin_id"
          class="text-[12px] min-w-0"
        >
          <p class="break-words">{{ plugin.plugin_id }} · {{ plugin.version }}</p>
          <output class="text-[11px] text-muted">{{
            $t(`plugins.manager.restoreState.${plugin.state}`)
          }}</output>
          <p v-if="plugin.error" role="alert" class="text-[11px] text-consumption break-words">
            {{ plugin.error }}
          </p>
          <template v-if="isUninstalledConfigured(plugin.plugin_id)">
            <UiButton
              class="mt-1"
              :disabled="!canManage"
              @click="requestConfiguredRemoval(plugin.plugin_id)"
            >
              {{ $t('plugins.manager.removeConfiguration') }}
            </UiButton>
            <div
              v-if="confirmConfiguredRemoval?.plugin_id === plugin.plugin_id"
              class="classic-inset mt-2 p-2 flex flex-col gap-2"
            >
              <p class="break-words">
                {{
                  $t('plugins.manager.confirmRemoveConfiguration', {
                    plugin: plugin.plugin_id,
                    version: plugin.version,
                  })
                }}
              </p>
              <div class="flex flex-wrap gap-2">
                <UiButton
                  variant="danger"
                  :disabled="!canManage"
                  @click="removeConfigured(plugin.plugin_id)"
                  >{{ $t('plugins.manager.removeConfigurationConfirm') }}</UiButton
                >
                <UiButton :disabled="busy" @click="confirmConfiguredRemoval = null">{{
                  $t('plugins.manager.cancel')
                }}</UiButton>
              </div>
            </div>
          </template>
        </div>
        <UiButton :disabled="!canManage" @click="retryConfigured">
          {{ $t('plugins.manager.retryConfigured') }}
        </UiButton>
      </section>
      <p v-if="snapshot.ready && !snapshot.installation_available" class="text-[12px] text-muted">
        {{ $t('plugins.manager.noPublishers') }}
      </p>
      <div class="flex flex-wrap gap-2">
        <UiButton :disabled="!canInstall" @click="pickPackage">
          {{ $t('plugins.manager.choosePackage') }}
        </UiButton>
        <UiButton :disabled="!canManage" @click="openRetainedData">
          {{ $t('plugins.manager.storedData') }}
        </UiButton>
      </div>

      <RetainedPluginData
        v-if="retainedDataOpened"
        :controller="retainedData"
        :disabled="!canManage"
      />

      <section v-if="preview" class="classic-card p-3 flex flex-col gap-3">
        <h3 class="classic-subsection-title">{{ $t('plugins.manager.reviewPackage') }}</h3>
        <dl class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-[12px] break-words min-w-0">
          <dt class="text-muted">{{ $t('plugins.manager.plugin') }}</dt>
          <dd>{{ preview.plugin_id }}</dd>
          <dt class="text-muted">{{ $t('plugins.manager.version') }}</dt>
          <dd>{{ preview.version }}</dd>
          <template v-if="preview.current_version">
            <dt class="text-muted">{{ $t('plugins.manager.currentVersion') }}</dt>
            <dd>{{ preview.current_version }}</dd>
          </template>
          <dt class="text-muted">{{ $t('plugins.manager.publisher') }}</dt>
          <dd>{{ preview.publisher_key_id }}</dd>
          <dt class="text-muted">{{ $t('plugins.manager.platform') }}</dt>
          <dd>{{ preview.target }}</dd>
        </dl>
        <div>
          <p class="classic-label">{{ $t('plugins.manager.permissions') }}</p>
          <ul v-if="preview.permissions.length" class="list-disc pl-4 text-[12px] break-words">
            <li v-for="permission in preview.permissions" :key="permission">
              {{ permissionLabel(permission) }}
            </li>
          </ul>
          <p v-else class="text-[12px] text-muted">{{ $t('plugins.manager.noPermissions') }}</p>
          <p class="text-[11px] text-muted mt-1">{{ $t('plugins.manager.permissionHelp') }}</p>
        </div>
        <label class="flex items-center gap-2 text-[12px]">
          <input v-model="enableAfterInstall" type="checkbox" :disabled="busy" />
          {{ $t('plugins.manager.enableAfterInstall') }}
        </label>
        <div class="flex flex-wrap gap-2">
          <UiButton variant="primary" :disabled="!canInstall" @click="install">
            {{ $t(preview.current_version ? 'plugins.manager.update' : 'plugins.manager.install') }}
          </UiButton>
          <UiButton :disabled="busy" @click="clearPreview">{{
            $t('plugins.manager.cancel')
          }}</UiButton>
        </div>
      </section>

      <p v-if="snapshot.ready && !snapshot.plugins.length" class="text-[12px] text-muted">
        {{ $t('plugins.manager.empty') }}
      </p>
      <article
        v-for="{ plugin, connection } in installedPlugins"
        :key="plugin.plugin_id"
        class="classic-card p-3 flex flex-col gap-2"
      >
        <div class="flex flex-wrap justify-between gap-2 text-[12px]">
          <h3 class="font-semibold break-words min-w-0">{{ plugin.plugin_id }}</h3>
          <span class="text-muted">{{ plugin.version }}</span>
        </div>
        <output class="text-[11px] text-muted">{{ stateLabel(plugin) }}</output>
        <output
          v-if="connection"
          class="block min-w-0 truncate text-[11px]"
          :class="connectionClasses[connection.tone]"
          :title="$t('plugins.manager.connection', { status: connection.value })"
          :data-plugin-connection="plugin.plugin_id"
        >
          {{ $t('plugins.manager.connection', { status: connection.value }) }}
        </output>
        <p v-if="plugin.configuration_managed" class="text-[11px] text-muted">
          {{ $t('plugins.manager.configurationManaged') }}
        </p>
        <p v-if="plugin.error" role="alert" class="text-[11px] text-consumption break-words">
          {{ plugin.error }}
        </p>
        <div class="flex flex-wrap gap-2">
          <UiButton
            v-if="plugin.permissions.includes('plugin_configuration')"
            :disabled="!canManage"
            @click="openSettings(plugin.plugin_id)"
          >
            {{ $t('plugins.manager.settings') }}
          </UiButton>
          <UiButton :disabled="!canManage" @click="setEnabled(plugin.plugin_id, !plugin.enabled)">
            {{ $t(plugin.enabled ? 'plugins.manager.disable' : 'plugins.manager.enable') }}
          </UiButton>
          <UiButton
            v-if="plugin.rollback_version"
            :disabled="!canManage || plugin.configuration_managed"
            @click="rollback(plugin.plugin_id)"
          >
            {{ $t('plugins.manager.rollback', { version: plugin.rollback_version }) }}
          </UiButton>
          <UiButton :disabled="!canManage" @click="requestRemoval(plugin.plugin_id)">
            {{ $t('plugins.manager.remove') }}
          </UiButton>
        </div>
        <PluginSettingsEditor
          v-if="settingsEditor?.plugin_id === plugin.plugin_id"
          :key="settingsEditor.key"
          :plugin-id="settingsEditor.plugin_id"
          :version="settingsEditor.version"
          :editor-key="settingsEditor.key"
          @busy="setSettingsBusy"
          @close="closeSettings"
          @saved="settingsSaved"
        />
        <div
          v-if="confirmRemoval === plugin.plugin_id"
          class="classic-inset p-2 flex flex-col gap-2"
        >
          <p class="text-[12px] break-words">
            {{
              $t(
                plugin.configuration_managed
                  ? 'plugins.manager.confirmRemoveConfigured'
                  : 'plugins.manager.confirmRemove',
                { plugin: plugin.plugin_id, version: plugin.version }
              )
            }}
          </p>
          <label class="flex items-center gap-2 text-[12px]">
            <input
              v-model="deleteSettings"
              type="checkbox"
              :disabled="busy"
              name="delete-plugin-settings"
            />
            {{ $t('plugins.manager.deleteSettings') }}
          </label>
          <p class="text-[11px] text-muted">{{ $t('plugins.manager.retainSettings') }}</p>
          <div class="flex gap-2">
            <UiButton
              variant="danger"
              :disabled="!canManage"
              @click="uninstall(plugin.plugin_id)"
              >{{ $t('plugins.manager.confirm') }}</UiButton
            >
            <UiButton :disabled="busy" @click="confirmRemoval = null">{{
              $t('plugins.manager.cancel')
            }}</UiButton>
          </div>
        </div>
      </article>
    </template>
  </section>
</template>

<script setup lang="ts">
import { computed, onMounted, onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'
import UiButton from '../../../components/UiButton.vue'
import PluginSettingsEditor from './PluginSettingsEditor.vue'
import RetainedPluginData from './RetainedPluginData.vue'
import { createPluginManager } from './usePluginManager'
import type { DashboardContribution, ManagedPlugin } from './types'

const { t: $t } = useI18n()
const manager = createPluginManager(() => $t('plugins.manager.operationFailed'))
const {
  snapshot,
  preview,
  enableAfterInstall,
  confirmRemoval,
  confirmConfiguredRemoval,
  deleteSettings,
  settingsEditor,
  retainedData,
  retainedDataOpened,
  working: busy,
  loading,
  error,
  installFailed,
  hasCurrentSnapshot,
  canManage,
  canInstall,
  retry,
  retryConfigured,
  pickPackage,
  clearPreview,
  install,
  setEnabled,
  rollback,
  requestRemoval,
  uninstall,
  isUninstalledConfigured,
  requestConfiguredRemoval,
  removeConfigured,
  openSettings,
  setSettingsBusy,
  closeSettings,
  settingsSaved,
  openRetainedData,
} = manager
const installedPlugins = computed(() =>
  (snapshot.value?.plugins ?? []).map((plugin) => ({
    plugin,
    // The reserved connection status ID is the only legacy summary shown here.
    // Other status contributions describe entities and do not belong in settings.
    connection:
      hasCurrentSnapshot.value &&
      plugin.enabled &&
      !plugin.error &&
      plugin.runtime?.state === 'running'
        ? plugin.runtime.contributions.find(
            (item): item is Extract<DashboardContribution, { kind: 'status' }> =>
              item.kind === 'status' && item.id === 'connection'
          )
        : undefined,
  }))
)
const connectionClasses = {
  neutral: 'text-muted',
  success: 'text-battery',
  warning: 'text-solar',
  error: 'text-consumption',
}
const permissionKeys: Record<string, string> = {
  dashboard_contributions: 'plugins.manager.permissionDashboard',
  plugin_configuration: 'plugins.manager.permissionConfiguration',
  network_http: 'plugins.manager.permissionHttp',
  network_mqtt: 'plugins.manager.permissionMqtt',
  desktop_notifications: 'plugins.manager.permissionNotifications',
  http_video: 'plugins.manager.permissionVideo',
  live_view: 'plugins.manager.permissionLiveView',
}
function permissionLabel(permission: string) {
  const key = permissionKeys[permission]
  return typeof key === 'string' ? $t(key) : permission
}
function stateLabel(plugin: ManagedPlugin) {
  if (!plugin.enabled) return $t('plugins.manager.disabled')
  if (plugin.error || plugin.runtime?.state === 'failed') return $t('plugins.manager.failed')
  switch (plugin.runtime?.state) {
    case 'running':
      return $t('plugins.manager.running')
    case 'starting':
    case 'restarting':
      return $t('plugins.manager.starting')
    default:
      return $t('plugins.manager.installed')
  }
}
onMounted(() => void manager.start())
onUnmounted(manager.stop)
</script>
