<template>
  <div
    class="h-screen bg-background-light dark:bg-[#0a0a0a] text-slate-800 dark:text-slate-200 flex flex-col font-sans select-none overflow-hidden"
  >
    <!-- macOS style titlebar (simulated) -->
    <div
      class="h-[36px] flex items-center justify-between px-3 border-b border-slate-200 dark:border-slate-800 bg-white dark:bg-black shadow-sm"
    >
      <div class="flex items-center gap-2">
        <Settings :size="14" class="text-slate-900 dark:text-slate-300" />
        <span
          class="text-[11px] font-bold tracking-tight uppercase text-slate-900 dark:text-slate-100"
          >Configuration</span
        >
      </div>
      <div class="flex items-center gap-1.5">
        <button
          @click="handleReset"
          class="p-1 rounded hover:bg-slate-100 dark:hover:bg-slate-800 transition-colors text-slate-900 dark:text-slate-300"
          title="Reset to defaults"
        >
          <RotateCcw :size="12" />
        </button>
        <button
          @click="handleSave"
          :disabled="saving"
          class="classic-btn !h-[18px] !text-[7px] !bg-accent !border-emerald-600 !text-white flex items-center gap-1 shadow-md"
          title="Save changes"
        >
          <Save :size="10" v-if="!saving" />
          <Loader2 :size="10" v-else class="animate-spin" />
          <span>SAVE</span>
        </button>
        <button
          @click="handleClose"
          class="p-1 rounded hover:bg-red-500 hover:text-white transition-colors text-slate-900 dark:text-slate-300"
        >
          <X :size="12" />
        </button>
      </div>
    </div>

    <!-- Main Layout -->
    <div class="flex-1 flex overflow-hidden">
      <!-- Sidebar -->
      <div
        class="w-[160px] border-r border-slate-200 dark:border-slate-800 bg-slate-50 dark:bg-black p-1.5 flex flex-col gap-1"
      >
        <button
          v-for="s in sections"
          :key="s.id"
          @click="activeTab = s.id"
          class="flex items-center gap-2 px-2.5 py-1.5 rounded text-[12px] font-bold transition-all uppercase tracking-tight"
          :class="
            activeTab === s.id
              ? 'bg-white dark:bg-slate-800 border border-slate-200 dark:border-slate-700 text-slate-800 dark:text-white shadow-sm'
              : 'text-slate-600 dark:text-slate-500 hover:text-slate-900 dark:hover:text-slate-200'
          "
        >
          <component :is="s.icon" :size="14" />
          {{ s.label }}
        </button>
      </div>

      <!-- Content Area -->
      <div class="flex-1 overflow-y-auto p-5 bg-white dark:bg-[#121212]">
        <div class="max-w-xl mx-auto flex flex-col gap-6">
          <!-- MQTT Section -->
          <div v-if="activeTab === 'mqtt'" class="flex flex-col gap-4">
            <header class="border-b border-slate-200 dark:border-slate-700 pb-2">
              <h2
                class="text-sm font-bold uppercase tracking-widest text-slate-900 dark:text-slate-100"
              >
                Broker Settings
              </h2>
            </header>

            <div class="grid grid-cols-2 gap-3">
              <div class="flex flex-col gap-1">
                <label
                  for="mqtt_host"
                  class="text-[10px] font-bold uppercase tracking-wider text-slate-800 dark:text-slate-400 px-1"
                  >Host</label
                >
                <input
                  id="mqtt_host"
                  v-model="config.mqtt_host"
                  type="text"
                  class="classic-input w-full"
                  placeholder="Cerbo.local"
                />
              </div>
              <div class="flex flex-col gap-1">
                <label
                  for="mqtt_port"
                  class="text-[10px] font-bold uppercase tracking-wider text-slate-800 dark:text-slate-400 px-1"
                  >Port</label
                >
                <input
                  id="mqtt_port"
                  v-model.number="config.mqtt_port"
                  type="number"
                  class="classic-input w-full"
                />
              </div>
              <div class="flex flex-col gap-1">
                <label
                  for="mqtt_login"
                  class="text-[10px] font-bold uppercase tracking-wider text-slate-800 dark:text-slate-400 px-1"
                  >Username</label
                >
                <input
                  id="mqtt_login"
                  v-model="config.mqtt_login"
                  type="text"
                  class="classic-input w-full"
                  placeholder="Optional"
                />
              </div>
              <div class="flex flex-col gap-1">
                <label
                  for="mqtt_password"
                  class="text-[10px] font-bold uppercase tracking-wider text-slate-800 dark:text-slate-400 px-1"
                  >Password</label
                >
                <input
                  id="mqtt_password"
                  v-model="config.mqtt_password"
                  type="password"
                  class="classic-input w-full"
                  placeholder="Optional"
                />
              </div>
            </div>

            <div class="flex flex-col gap-1">
              <label
                for="portal_id"
                class="text-[10px] font-bold uppercase tracking-wider text-slate-800 dark:text-slate-400 px-1"
                >VRM Portal ID</label
              >
              <input
                id="portal_id"
                v-model="config.portal_id"
                type="text"
                class="classic-input w-full"
                placeholder="e.g. a1b2c3d4e5f6"
              />
              <p class="text-[10px] text-slate-500 dark:text-slate-500 px-1 italic">
                Keep-alive for Cerbo GX.
              </p>
            </div>

            <div class="flex flex-col gap-2" role="radiogroup" aria-label="Interface Theme">
              <span
                class="text-[10px] font-bold uppercase tracking-wider text-slate-800 dark:text-slate-400 px-1"
                >Interface Theme</span
              >
              <div class="flex gap-1">
                <button
                  @click="config.color_scheme = 'dark'"
                  class="classic-btn !normal-case flex-1"
                  :class="{ 'classic-btn-on': config.color_scheme === 'dark' }"
                >
                  Dark
                </button>
                <button
                  @click="config.color_scheme = 'light'"
                  class="classic-btn !normal-case flex-1"
                  :class="{ 'classic-btn-on': config.color_scheme === 'light' }"
                >
                  Light
                </button>
              </div>
            </div>
          </div>

          <!-- Home Assistant Section -->
          <div v-if="activeTab === 'ha'" class="flex flex-col gap-4">
            <header class="border-b border-slate-200 dark:border-slate-700 pb-2">
              <h2
                class="text-sm font-bold uppercase tracking-widest text-slate-900 dark:text-slate-100"
              >
                Home Assistant
              </h2>
            </header>

            <div
              class="flex flex-col gap-3 p-3 bg-slate-50 dark:bg-black rounded border border-slate-200 dark:border-slate-700"
            >
              <div class="flex flex-col gap-1">
                <label
                  for="ha_url"
                  class="text-[10px] font-bold uppercase tracking-wider text-slate-800 dark:text-slate-400 px-1"
                  >Server URL</label
                >
                <input
                  id="ha_url"
                  v-model="config.ha_url"
                  type="text"
                  class="classic-input w-full"
                  placeholder="http://homeassistant.local"
                />
              </div>
              <div class="grid grid-cols-2 gap-3">
                <div class="flex flex-col gap-1">
                  <label
                    for="ha_port"
                    class="text-[10px] font-bold uppercase tracking-wider text-slate-800 dark:text-slate-400 px-1"
                    >API Port</label
                  >
                  <input
                    id="ha_port"
                    v-model.number="config.ha_port"
                    type="number"
                    class="classic-input w-full"
                    placeholder="8123"
                  />
                </div>
                <div class="flex flex-col gap-1">
                  <span
                    class="text-[10px] font-bold uppercase tracking-wider text-slate-800 dark:text-slate-400 px-1"
                    >Status</span
                  >
                  <div
                    class="h-8 flex items-center px-2 rounded border border-slate-200 dark:border-slate-700 bg-white dark:bg-[#1a1a1a] text-[10px] font-bold"
                  >
                    <span
                      :class="
                        haDirectMonitoringEnabled
                          ? 'text-green-500'
                          : 'text-slate-600 dark:text-slate-500'
                      "
                    >
                      API: {{ haDirectMonitoringEnabled ? 'Enabled' : 'Disabled' }}
                    </span>
                  </div>
                </div>
              </div>
              <div class="flex flex-col gap-1">
                <label
                  for="ha_token"
                  class="text-[10px] font-bold uppercase tracking-wider text-slate-800 dark:text-slate-400 px-1"
                  >Access Token</label
                >
                <input
                  id="ha_token"
                  v-model="config.ha_longlived_token"
                  type="password"
                  class="classic-input w-full"
                  placeholder="Token"
                />
              </div>

              <div class="flex gap-2 mt-1">
                <button
                  @click="testHaConnection"
                  :disabled="testingHa"
                  class="classic-btn flex-1 !normal-case"
                >
                  {{ testingHa ? 'Testing...' : 'Test Connection' }}
                </button>
                <button
                  @click="handleFetchHaEntities"
                  :disabled="discoveryLoading || !isHaConfigured"
                  class="classic-btn flex-1 !normal-case"
                >
                  Fetch Entities
                </button>
              </div>

              <div
                v-if="haTestResult"
                :class="haTestSuccess ? 'text-green-500' : 'text-red-500'"
                class="text-[10px] font-bold text-center mt-1"
              >
                {{ haTestResult }}
              </div>
            </div>

            <div class="flex flex-col gap-3 mt-2">
              <h3
                class="text-[10px] font-bold uppercase tracking-widest text-slate-900 dark:text-slate-100"
              >
                MQTT Routing
              </h3>
              <div class="grid grid-cols-2 gap-3">
                <div class="flex flex-col gap-1">
                  <label
                    for="mqtt_ha_host"
                    class="text-[10px] font-bold uppercase tracking-wider text-slate-800 dark:text-slate-400 px-1"
                    >HA MQTT Host</label
                  >
                  <input
                    id="mqtt_ha_host"
                    v-model="config.mqtt_ha_host"
                    type="text"
                    class="classic-input w-full"
                  />
                </div>
                <div class="flex flex-col gap-1">
                  <label
                    for="mqtt_ha_port"
                    class="text-[10px] font-bold uppercase tracking-wider text-slate-800 dark:text-slate-400 px-1"
                    >HA MQTT Port</label
                  >
                  <input
                    id="mqtt_ha_port"
                    v-model.number="config.mqtt_ha_port"
                    type="number"
                    class="classic-input w-full"
                  />
                </div>
              </div>
              <div class="grid grid-cols-2 gap-3">
                <div class="flex flex-col gap-1">
                  <label
                    for="mqtt_ha_login"
                    class="text-[10px] font-bold uppercase tracking-wider text-slate-800 dark:text-slate-400 px-1"
                    >HA MQTT Username</label
                  >
                  <input
                    id="mqtt_ha_login"
                    v-model="config.mqtt_ha_login"
                    type="text"
                    class="classic-input w-full"
                    placeholder="Optional"
                  />
                </div>
                <div class="flex flex-col gap-1">
                  <label
                    for="mqtt_ha_password"
                    class="text-[10px] font-bold uppercase tracking-wider text-slate-800 dark:text-slate-400 px-1"
                    >HA MQTT Password</label
                  >
                  <input
                    id="mqtt_ha_password"
                    v-model="config.mqtt_ha_password"
                    type="password"
                    class="classic-input w-full"
                    placeholder="Optional"
                  />
                </div>
              </div>
              <div class="flex flex-col gap-1">
                <span
                  class="text-[10px] font-bold uppercase tracking-wider text-slate-800 dark:text-slate-400 px-1"
                  >Camera Monitoring</span
                >
                <label
                  class="flex items-center gap-2 px-2 py-1.5 rounded border border-slate-200 dark:border-slate-700 bg-white dark:bg-[#1a1a1a] cursor-pointer group hover:border-accent/30 transition-colors"
                >
                  <input
                    type="checkbox"
                    v-model="config.camera_enabled"
                    class="rounded border-slate-300 text-accent focus:ring-accent"
                  />
                  <span class="text-[11px] font-bold text-slate-700 dark:text-slate-300"
                    >Enable camera event detection</span
                  >
                </label>
              </div>
              <div class="flex flex-col gap-1">
                <label
                  for="camera_topic"
                  class="text-[10px] font-bold uppercase tracking-wider text-slate-800 dark:text-slate-400 px-1"
                  >Camera Detection Topic</label
                >
                <input
                  id="camera_topic"
                  v-model="config.camera_topic"
                  type="text"
                  :disabled="!config.camera_enabled"
                  class="classic-input w-full disabled:opacity-50"
                  placeholder="e.g. frigate/+/events"
                />
                <p class="text-[10px] text-slate-500 dark:text-slate-500 px-1 italic">
                  MQTT topic with wildcard for camera events on HA broker.
                </p>
              </div>
            </div>
          </div>

          <!-- Sections Visibility -->
          <div v-if="activeTab === 'sections'" class="flex flex-col gap-4">
            <header class="border-b border-slate-200 dark:border-slate-700 pb-2">
              <h2
                class="text-sm font-bold uppercase tracking-widest text-slate-900 dark:text-slate-100"
              >
                Section Visibility
              </h2>
            </header>

            <!-- Group 1: Inverter & Solar -->
            <div
              class="flex flex-col gap-2 p-3 bg-slate-50 dark:bg-black rounded border border-slate-200 dark:border-slate-700"
            >
              <h3
                class="text-[10px] font-bold uppercase tracking-widest text-slate-900 dark:text-slate-100"
              >
                Inverter & Solar
              </h3>
              <label
                class="flex items-center gap-2 px-2 py-1.5 rounded border border-slate-200 dark:border-slate-700 bg-white dark:bg-[#1a1a1a] cursor-pointer group hover:border-accent/30 transition-colors"
              >
                <input
                  type="checkbox"
                  v-model="config.show_batteries"
                  class="rounded border-slate-300 text-accent focus:ring-accent"
                />
                <span class="text-[11px] font-bold text-slate-700 dark:text-slate-300"
                  >Batteries</span
                >
              </label>
              <label
                class="flex items-center gap-2 px-2 py-1.5 rounded border border-slate-200 dark:border-slate-700 bg-white dark:bg-[#1a1a1a] cursor-pointer group hover:border-accent/30 transition-colors"
              >
                <input
                  type="checkbox"
                  v-model="config.show_solar_production"
                  class="rounded border-slate-300 text-accent focus:ring-accent"
                />
                <span class="text-[11px] font-bold text-slate-700 dark:text-slate-300"
                  >Solar Production</span
                >
              </label>
            </div>

            <!-- Group 2: Energy Stats -->
            <div
              class="flex flex-col gap-2 p-3 bg-slate-50 dark:bg-black rounded border border-slate-200 dark:border-slate-700"
            >
              <h3
                class="text-[10px] font-bold uppercase tracking-widest text-slate-900 dark:text-slate-100"
              >
                Energy Stats
              </h3>
              <label
                class="flex items-center gap-2 px-2 py-1.5 rounded border border-slate-200 dark:border-slate-700 bg-white dark:bg-[#1a1a1a] cursor-pointer group hover:border-accent/30 transition-colors"
              >
                <input
                  type="checkbox"
                  v-model="config.show_active_loads"
                  class="rounded border-slate-300 text-accent focus:ring-accent"
                />
                <span class="text-[11px] font-bold text-slate-700 dark:text-slate-300"
                  >Active Loads</span
                >
              </label>
              <label
                class="flex items-center gap-2 px-2 py-1.5 rounded border border-slate-200 dark:border-slate-700 bg-white dark:bg-[#1a1a1a] cursor-pointer group hover:border-accent/30 transition-colors"
              >
                <input
                  type="checkbox"
                  v-model="config.show_daily_stats"
                  class="rounded border-slate-300 text-accent focus:ring-accent"
                />
                <span class="text-[11px] font-bold text-slate-700 dark:text-slate-300"
                  >Daily Stats</span
                >
              </label>
            </div>

            <!-- Group 3: Home Area -->
            <div
              class="flex flex-col gap-2 p-3 bg-slate-50 dark:bg-black rounded border border-slate-200 dark:border-slate-700"
            >
              <h3
                class="text-[10px] font-bold uppercase tracking-widest text-slate-900 dark:text-slate-100"
              >
                Home Area
              </h3>
              <label
                class="flex items-center gap-2 px-2 py-1.5 rounded border border-slate-200 dark:border-slate-700 bg-white dark:bg-[#1a1a1a] cursor-pointer group hover:border-accent/30 transition-colors"
              >
                <input
                  type="checkbox"
                  v-model="config.show_ev"
                  class="rounded border-slate-300 text-accent focus:ring-accent"
                />
                <span class="text-[11px] font-bold text-slate-700 dark:text-slate-300">EV</span>
              </label>
              <label
                class="flex items-center gap-2 px-2 py-1.5 rounded border border-slate-200 dark:border-slate-700 bg-white dark:bg-[#1a1a1a] cursor-pointer group hover:border-accent/30 transition-colors"
              >
                <input
                  type="checkbox"
                  v-model="config.show_washer"
                  class="rounded border-slate-300 text-accent focus:ring-accent"
                />
                <span class="text-[11px] font-bold text-slate-700 dark:text-slate-300">Washer</span>
              </label>
              <label
                class="flex items-center gap-2 px-2 py-1.5 rounded border border-slate-200 dark:border-slate-700 bg-white dark:bg-[#1a1a1a] cursor-pointer group hover:border-accent/30 transition-colors"
              >
                <input
                  type="checkbox"
                  v-model="config.show_dryer"
                  class="rounded border-slate-300 text-accent focus:ring-accent"
                />
                <span class="text-[11px] font-bold text-slate-700 dark:text-slate-300">Dryer</span>
              </label>
              <label
                class="flex items-center gap-2 px-2 py-1.5 rounded border border-slate-200 dark:border-slate-700 bg-white dark:bg-[#1a1a1a] cursor-pointer group hover:border-accent/30 transition-colors"
              >
                <input
                  type="checkbox"
                  v-model="config.show_dishwasher"
                  class="rounded border-slate-300 text-accent focus:ring-accent"
                />
                <span class="text-[11px] font-bold text-slate-700 dark:text-slate-300"
                  >Dishwasher</span
                >
              </label>
              <label
                class="flex items-center gap-2 px-2 py-1.5 rounded border border-slate-200 dark:border-slate-700 bg-white dark:bg-[#1a1a1a] cursor-pointer group hover:border-accent/30 transition-colors"
              >
                <input
                  type="checkbox"
                  v-model="config.show_home_section"
                  class="rounded border-slate-300 text-accent focus:ring-accent"
                />
                <span class="text-[11px] font-bold text-slate-700 dark:text-slate-300"
                  >Home Buttons</span
                >
              </label>
              <label
                class="flex items-center gap-2 px-2 py-1.5 rounded border border-slate-200 dark:border-slate-700 bg-white dark:bg-[#1a1a1a] cursor-pointer group hover:border-accent/30 transition-colors"
              >
                <input
                  type="checkbox"
                  v-model="config.show_header_toggles"
                  class="rounded border-slate-300 text-accent focus:ring-accent"
                />
                <span class="text-[11px] font-bold text-slate-700 dark:text-slate-300">{{
                  $t('config.headerToggles')
                }}</span>
              </label>
              <label
                class="flex items-center gap-2 px-2 py-1.5 rounded border border-slate-200 dark:border-slate-700 bg-white dark:bg-[#1a1a1a] cursor-pointer group hover:border-accent/30 transition-colors"
              >
                <input
                  type="checkbox"
                  v-model="config.show_ha_sensors"
                  class="rounded border-slate-300 text-accent focus:ring-accent"
                />
                <span class="text-[11px] font-bold text-slate-700 dark:text-slate-300">{{
                  $t('config.sensors')
                }}</span>
              </label>
              <label
                class="flex items-center gap-2 px-2 py-1.5 rounded border border-slate-200 dark:border-slate-700 bg-white dark:bg-[#1a1a1a] cursor-pointer group hover:border-accent/30 transition-colors"
              >
                <input
                  type="checkbox"
                  v-model="config.show_ha_numbers"
                  class="rounded border-slate-300 text-accent focus:ring-accent"
                />
                <span class="text-[11px] font-bold text-slate-700 dark:text-slate-300">{{
                  $t('config.numbers')
                }}</span>
              </label>
              <label
                class="flex items-center gap-2 px-2 py-1.5 rounded border border-slate-200 dark:border-slate-700 bg-white dark:bg-[#1a1a1a] cursor-pointer group hover:border-accent/30 transition-colors"
              >
                <input
                  type="checkbox"
                  v-model="config.show_ha_covers"
                  class="rounded border-slate-300 text-accent focus:ring-accent"
                />
                <span class="text-[11px] font-bold text-slate-700 dark:text-slate-300">{{
                  $t('config.covers')
                }}</span>
              </label>
              <label
                class="flex items-center gap-2 px-2 py-1.5 rounded border border-slate-200 dark:border-slate-700 bg-white dark:bg-[#1a1a1a] cursor-pointer group hover:border-accent/30 transition-colors"
              >
                <input
                  type="checkbox"
                  v-model="config.show_ha_media"
                  class="rounded border-slate-300 text-accent focus:ring-accent"
                />
                <span class="text-[11px] font-bold text-slate-700 dark:text-slate-300">{{
                  $t('config.mediaPlayers')
                }}</span>
              </label>
              <label
                class="flex items-center gap-2 px-2 py-1.5 rounded border border-slate-200 dark:border-slate-700 bg-white dark:bg-[#1a1a1a] cursor-pointer group hover:border-accent/30 transition-colors"
              >
                <input
                  type="checkbox"
                  v-model="config.show_ha_scenes"
                  class="rounded border-slate-300 text-accent focus:ring-accent"
                />
                <span class="text-[11px] font-bold text-slate-700 dark:text-slate-300">{{
                  $t('config.scenes')
                }}</span>
              </label>
              <label
                class="flex items-center gap-2 px-2 py-1.5 rounded border border-slate-200 dark:border-slate-700 bg-white dark:bg-[#1a1a1a] cursor-pointer group hover:border-accent/30 transition-colors"
              >
                <input
                  type="checkbox"
                  v-model="config.show_ha_weather"
                  class="rounded border-slate-300 text-accent focus:ring-accent"
                />
                <span class="text-[11px] font-bold text-slate-700 dark:text-slate-300">{{
                  $t('config.weather')
                }}</span>
              </label>
              <label
                class="flex items-center gap-2 px-2 py-1.5 rounded border border-slate-200 dark:border-slate-700 bg-white dark:bg-[#1a1a1a] cursor-pointer group hover:border-accent/30 transition-colors"
              >
                <input
                  type="checkbox"
                  v-model="config.show_console"
                  class="rounded border-slate-300 text-accent focus:ring-accent"
                />
                <span class="text-[11px] font-bold text-slate-700 dark:text-slate-300"
                  >Console</span
                >
              </label>

              <!-- App Settings -->
              <div
                class="flex flex-col gap-2 p-3 bg-slate-50 dark:bg-black rounded border border-slate-200 dark:border-slate-700"
              >
                <h3
                  class="text-[10px] font-bold uppercase tracking-widest text-slate-900 dark:text-slate-100"
                >
                  App Settings
                </h3>
                <label
                  class="flex items-center gap-2 px-2 py-1.5 rounded border border-slate-200 dark:border-slate-700 bg-white dark:bg-[#1a1a1a] cursor-pointer group hover:border-accent/30 transition-colors"
                >
                  <input
                    type="checkbox"
                    :checked="config.auto_start"
                    @change="config.auto_start = ($event.target as HTMLInputElement).checked"
                    class="rounded border-slate-300 text-accent focus:ring-accent"
                  />
                  <span class="text-[11px] font-bold text-slate-700 dark:text-slate-300">
                    Launch at system startup
                  </span>
                </label>
              </div>

              <!-- Authentication -->
              <div
                class="flex flex-col gap-2 p-3 bg-slate-50 dark:bg-black rounded border border-slate-200 dark:border-slate-700"
              >
                <h3
                  class="text-[10px] font-bold uppercase tracking-widest text-slate-900 dark:text-slate-100"
                >
                  Authentication
                </h3>
                <label
                  class="flex items-center gap-2 px-2 py-1.5 rounded border border-slate-200 dark:border-slate-700 bg-white dark:bg-[#1a1a1a] cursor-pointer group hover:border-accent/30 transition-colors"
                >
                  <input
                    type="checkbox"
                    :checked="config.auth_enabled"
                    @change="config.auth_enabled = ($event.target as HTMLInputElement).checked"
                    class="rounded border-slate-300 text-accent focus:ring-accent"
                  />
                  <span class="text-[11px] font-bold text-slate-700 dark:text-slate-300">
                    Enable authentication
                  </span>
                </label>
                <div v-if="config.auth_enabled" class="flex flex-col gap-2 mt-1">
                  <div class="flex flex-col gap-1">
                    <label for="auth_username" class="text-[10px] font-medium text-slate-500"
                      >Username</label
                    >
                    <input
                      id="auth_username"
                      type="text"
                      v-model="config.auth_username"
                      placeholder="Enter username"
                      class="rounded border border-slate-300 dark:border-slate-600 bg-white dark:bg-[#1a1a1a] px-2 py-1 text-[11px] text-slate-700 dark:text-slate-300"
                    />
                  </div>
                  <div class="flex flex-col gap-1">
                    <label for="auth_password" class="text-[10px] font-medium text-slate-500"
                      >Password</label
                    >
                    <input
                      id="auth_password"
                      type="password"
                      v-model="config.auth_password"
                      placeholder="Enter password"
                      class="rounded border border-slate-300 dark:border-slate-600 bg-white dark:bg-[#1a1a1a] px-2 py-1 text-[11px] text-slate-700 dark:text-slate-300"
                    />
                  </div>
                  <label
                    class="flex items-center gap-2 px-2 py-1.5 rounded border border-slate-200 dark:border-slate-700 bg-white dark:bg-[#1a1a1a] cursor-pointer group hover:border-accent/30 transition-colors"
                  >
                    <input
                      type="checkbox"
                      :checked="config.auth_biometric"
                      @change="config.auth_biometric = ($event.target as HTMLInputElement).checked"
                      class="rounded border-slate-300 text-accent focus:ring-accent"
                    />
                    <span class="text-[11px] font-bold text-slate-700 dark:text-slate-300">
                      Allow biometric authentication (Touch ID / Windows Hello)
                    </span>
                  </label>
                </div>
              </div>
            </div>
          </div>

          <!-- Entities Section -->
          <div v-if="activeTab === 'entities'" class="flex flex-col gap-6">
            <header class="border-b border-slate-200 dark:border-slate-700 pb-2">
              <h2
                class="text-sm font-bold uppercase tracking-widest text-slate-900 dark:text-slate-100"
              >
                UI Controls
              </h2>
            </header>

            <HaEntitiesEditor
              :haEntitiesList="haEntitiesList"
              :discoveredEntities="discoveredEntities"
              :entityRules="entityRules"
              @add="addHaEntity"
              @remove="removeHaEntity"
              @move-up="moveEntityUp"
              @move-down="moveEntityDown"
              @focus-entity="
                ensureEntitiesFetched(
                  config.ha_url || '',
                  config.ha_port,
                  config.ha_longlived_token || ''
                )
              "
            />

            <div class="h-px bg-slate-100 dark:bg-slate-800"></div>

            <HeaderTogglesEditor
              :headerTogglesList="headerTogglesList"
              :discoveredEntities="discoveredEntities"
              :entityRules="entityRules"
              @add="addHeaderToggle"
              @remove="removeHeaderToggle"
              @move-up="moveToggleUp"
              @move-down="moveToggleDown"
              @focus-entity="
                ensureEntitiesFetched(
                  config.ha_url || '',
                  config.ha_port,
                  config.ha_longlived_token || ''
                )
              "
            />
          </div>
        </div>
      </div>
    </div>

    <!-- Discovery Dialog (Custom) -->
    <div
      v-if="discoveryDialog"
      class="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/30 backdrop-blur-[2px]"
    >
      <div
        class="classic-card w-full max-w-sm max-h-[80vh] flex flex-col overflow-hidden dark:bg-[#121212] shadow-2xl animate-in fade-in duration-150"
      >
        <header
          class="p-3 border-b border-slate-200 dark:border-slate-700 flex items-center justify-between bg-slate-50 dark:bg-black"
        >
          <h3 class="text-xs font-bold uppercase text-slate-900 dark:text-slate-100">
            Discover Entities
          </h3>
          <button
            @click="discoveryDialog = false"
            class="text-slate-900 dark:text-slate-300 hover:text-slate-900 dark:hover:text-slate-200"
          >
            <X :size="16" />
          </button>
        </header>
        <div class="flex-1 overflow-y-auto p-2 flex flex-col gap-1">
          <div
            v-if="discoveryLoading"
            class="flex flex-col items-center justify-center py-10 gap-2"
          >
            <Loader2 class="animate-spin text-accent" :size="20" />
            <span class="text-[10px] font-bold text-slate-900 dark:text-slate-300 uppercase"
              >Fetching...</span
            >
          </div>
          <div
            v-else
            v-for="e in discoveredEntities"
            :key="e.entity_id"
            @click="toggleSelection(e.entity_id)"
            class="p-2 rounded border border-transparent cursor-pointer transition-all flex items-center justify-between group"
            :class="
              selectedDiscovery.includes(e.entity_id)
                ? 'bg-accent/10 border-accent/20'
                : 'hover:bg-slate-50 dark:hover:bg-slate-800'
            "
          >
            <div>
              <div
                class="text-[11px] font-bold group-hover:text-accent transition-colors"
                :class="{
                  'text-accent': selectedDiscovery.includes(e.entity_id),
                  'dark:text-slate-300': !selectedDiscovery.includes(e.entity_id),
                }"
              >
                {{ e.friendly_name }}
              </div>
              <div class="text-[9px] text-slate-500 dark:text-slate-500 font-mono">
                {{ e.entity_id }}
              </div>
            </div>
            <div v-if="selectedDiscovery.includes(e.entity_id)" class="text-accent">
              <Check :size="12" />
            </div>
          </div>
        </div>
        <footer
          class="p-3 border-t border-slate-200 dark:border-slate-700 flex flex-col gap-2 bg-slate-50 dark:bg-black"
        >
          <div class="flex gap-1 p-0.5 bg-slate-200/50 dark:bg-slate-800 rounded">
            <button
              @click="discoveryTargetGroup = 'home'"
              class="flex-1 py-1 rounded text-[9px] font-bold transition-all uppercase"
              :class="
                discoveryTargetGroup === 'home'
                  ? 'bg-white dark:bg-slate-700 shadow-sm dark:text-white'
                  : 'text-slate-500 opacity-50 dark:text-slate-400'
              "
            >
              Home Buttons
            </button>
            <button
              @click="discoveryTargetGroup = 'toggle'"
              class="flex-1 py-1 rounded text-[9px] font-bold transition-all uppercase"
              :class="
                discoveryTargetGroup === 'toggle'
                  ? 'bg-white dark:bg-slate-700 shadow-sm dark:text-white'
                  : 'text-slate-500 opacity-50 dark:text-slate-400'
              "
            >
              Header Toggles
            </button>
          </div>
          <div class="flex gap-2">
            <button @click="discoveryDialog = false" class="classic-btn flex-1 !normal-case">
              Cancel
            </button>
            <button
              @click="addDiscoveredEntities"
              :disabled="!selectedDiscovery.length"
              class="classic-btn !bg-accent !border-emerald-600 !text-white flex-1 !normal-case disabled:!bg-slate-300"
            >
              Add ({{ selectedDiscovery.length }})
            </button>
          </div>
        </footer>
      </div>
    </div>

    <!-- Toast Notification -->
    <div
      v-if="message"
      class="fixed bottom-4 left-1/2 -translate-x-1/2 z-[60] px-4 py-1.5 rounded-full shadow-lg text-[10px] font-bold border animate-in slide-in-from-bottom duration-200 uppercase tracking-wider"
      :class="
        messageType === 'error'
          ? 'bg-red-500 border-red-600 text-white'
          : 'bg-green-500 border-green-600 text-white'
      "
    >
      {{ message }}
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { invoke } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { emit } from '@tauri-apps/api/event'
import { logger } from './logger'

const { t: $t } = useI18n()
import {
  Settings,
  Wifi,
  Home,
  Layout,
  Eye,
  Save,
  RotateCcw,
  X,
  Loader2,
  Check,
} from '@lucide/vue'
import { useConfigForm } from './composables/useConfigForm'
import { useHAEntityManager } from './composables/useHAEntityManager'
import HaEntitiesEditor from './components/HaEntitiesEditor.vue'
import HeaderTogglesEditor from './components/HeaderTogglesEditor.vue'

const {
  config,
  saving,
  message,
  messageType,
  loadConfig,
  saveConfig,
  resetToDefaults,
  clearMessage,
} = useConfigForm()
const {
  haEntitiesList,
  headerTogglesList,
  discoveryDialog,
  discoveredEntities,
  selectedDiscovery,
  discoveryLoading,
  discoveryTargetGroup,
  loadFromConfig,
  fetchHaEntities,
  addDiscoveredEntities,
  addHaEntity,
  removeHaEntity,
  moveEntityUp,
  moveEntityDown,
  addHeaderToggle,
  removeHeaderToggle,
  moveToggleUp,
  moveToggleDown,
  ensureEntitiesFetched,
} = useHAEntityManager()

const activeTab = ref('mqtt')
const sections = [
  { id: 'mqtt', label: 'MQTT Broker', icon: Wifi },
  { id: 'ha', label: 'Home Assistant', icon: Home },
  { id: 'entities', label: 'UI Controls', icon: Layout },
  { id: 'sections', label: 'Sections', icon: Eye },
]

const testingHa = ref(false)
const haTestResult = ref('')
const haTestSuccess = ref(false)

const haDirectMonitoringEnabled = computed(() => {
  return !!(
    config.ha_url &&
    config.ha_longlived_token &&
    config.ha_url.trim() &&
    config.ha_longlived_token.trim()
  )
})

const isHaConfigured = computed(() => haDirectMonitoringEnabled.value)

watch(
  [() => config.ha_longlived_token, () => config.ha_url],
  ([token, url]) => {
    config.ha_use_direct_api = !!(token && url && token.trim() && url.trim())
  },
  { immediate: true }
)

const entityRules = [(v: string) => !!v || 'Required']

async function testHaConnection() {
  if (!config.ha_url || !config.ha_longlived_token) {
    message.value = 'URL and Token required'
    messageType.value = 'error'
    setTimeout(clearMessage, 3000)
    return
  }
  testingHa.value = true
  haTestResult.value = ''
  try {
    await invoke('test_ha_connection', {
      url: config.ha_url,
      port: config.ha_port || 8123,
      token: config.ha_longlived_token,
    })
    haTestResult.value = 'Connection successful'
    haTestSuccess.value = true
  } catch (e) {
    haTestResult.value = `Failed: ${e?.toString() || e}`
    haTestSuccess.value = false
  } finally {
    testingHa.value = false
  }
}

async function handleFetchHaEntities() {
  if (!config.ha_url || !config.ha_longlived_token) {
    message.value = 'Please enter HA URL and Token first'
    messageType.value = 'error'
    setTimeout(clearMessage, 3000)
    return
  }
  try {
    await fetchHaEntities(config.ha_url, config.ha_port, config.ha_longlived_token)
  } catch (e) {
    message.value = `Discovery failed: ${e?.toString() || e}`
    messageType.value = 'error'
    setTimeout(clearMessage, 3000)
  }
}

async function handleSave() {
  await saveConfig(haEntitiesList.value, headerTogglesList.value)
  // Apply auto-start setting
  try {
    await invoke('set_auto_start', { enable: config.auto_start ?? false })
  } catch (e) {
    logger.warn('Failed to set auto-start:', e)
  }
  await emit('config-saved', { color_scheme: config.color_scheme })
  message.value = 'Settings saved successfully'
  messageType.value = 'success'
  setTimeout(clearMessage, 2000)
}

async function handleClose() {
  try {
    const win = getCurrentWindow()
    await win.close()
  } catch (e) {
    console.warn('Frontend close failed, trying backend:', e)
    try {
      await invoke('close_config_window')
    } catch (err) {
      console.error('Close failed:', err)
    }
  }
}

function handleReset() {
  if (confirm('Reset all settings to defaults?')) {
    resetToDefaults()
    haEntitiesList.value = []
    headerTogglesList.value = []
  }
}

const toggleSelection = (id: string) => {
  const index = selectedDiscovery.value.indexOf(id)
  if (index > -1) selectedDiscovery.value.splice(index, 1)
  else selectedDiscovery.value.push(id)
}

const applyTheme = (scheme: string | null | undefined) => {
  const isDark = scheme === 'dark'
  console.log('Applying theme to Config window:', scheme, isDark)
  document.documentElement.classList.toggle('dark', isDark)
  document.body.classList.toggle('dark', isDark)

  // Force background to prevent system-level dark mode overrides if any
  if (isDark) {
    document.documentElement.style.backgroundColor = '#0a0a0a'
    document.body.style.backgroundColor = '#0a0a0a'
  } else {
    document.documentElement.style.backgroundColor = '#efeff4'
    document.body.style.backgroundColor = '#efeff4'
  }
}

watch(
  () => config.color_scheme,
  (scheme) => {
    applyTheme(scheme)
  },
  { immediate: true }
)

async function handleKeyDown(e: KeyboardEvent) {
  const isW = e.key === 'w' || e.key === 'W' || e.code === 'KeyW'
  if ((e.metaKey || e.ctrlKey) && isW) {
    e.preventDefault()
    e.stopPropagation()
    await handleClose()
  }
}

onMounted(async () => {
  try {
    globalThis.addEventListener('keydown', handleKeyDown)
    const cfg = await loadConfig()
    loadFromConfig(cfg)
    // Re-apply after loading to be absolutely sure
    applyTheme(cfg.color_scheme)
  } catch (err) {
    console.error('Config init failed:', err)
  }
})

onUnmounted(() => {
  globalThis.removeEventListener('keydown', handleKeyDown)
})
</script>
