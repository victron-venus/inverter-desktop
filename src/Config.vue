<template>
  <ErrorBoundary>
    <div class="app-shell h-screen flex flex-col select-none overflow-hidden">
      <!-- macOS style titlebar (simulated) -->
      <div class="config-titlebar h-[36px] flex items-center justify-between px-3">
        <div class="flex items-center gap-2">
          <Settings :size="14" class="text-muted" />
          <span class="text-[12px] font-semibold tracking-tight text-main">Configuration</span>
        </div>
        <div class="flex items-center gap-1.5">
          <button
            type="button"
            @click="handleReset"
            class="p-1 rounded-md hover:bg-black/[0.04] dark:hover:bg-white/[0.06] transition-colors text-muted hover:text-main"
            title="Reset to defaults"
          >
            <RotateCcw :size="12" />
          </button>
          <UiButton
            variant="primary"
            size="sm"
            class="!h-[22px] gap-1"
            :loading="saving"
            title="Save changes"
            @click="handleSave"
          >
            <Save v-if="!saving" :size="10" />
            <span>Save</span>
          </UiButton>
          <button
            type="button"
            @click="handleClose"
            class="p-1 rounded-md hover:bg-consumption hover:text-white transition-colors text-muted hover:text-main"
          >
            <X :size="12" />
          </button>
        </div>
      </div>

      <!-- Main Layout -->
      <div class="flex-1 flex overflow-hidden">
        <!-- Sidebar -->
        <div class="config-sidebar w-[160px] p-1.5 flex flex-col gap-0.5">
          <button
            type="button"
            v-for="s in sections"
            :key="s.id"
            @click="activeTab = s.id"
            class="config-nav-item"
            :class="activeTab === s.id ? 'config-nav-item-active' : ''"
          >
            <component :is="s.icon" :size="14" />
            {{ s.label }}
          </button>
        </div>

        <!-- Content Area -->
        <div class="flex-1 overflow-y-auto p-5 bg-[#f7f7f8] dark:bg-[#121214]">
          <div class="max-w-xl mx-auto flex flex-col gap-6">
            <!-- MQTT Section -->
            <div v-if="activeTab === 'mqtt'" class="flex flex-col gap-4">
              <header class="border-b border-black/[0.06] dark:border-white/[0.07] pb-2">
                <h2 class="classic-section-title">Broker Settings</h2>
              </header>

              <div class="grid grid-cols-2 gap-3">
                <div class="flex flex-col gap-1">
                  <label for="mqtt_host" class="classic-label px-1">Host</label>
                  <input
                    id="mqtt_host"
                    v-model="config.mqtt_host"
                    type="text"
                    class="classic-input w-full"
                    placeholder="Cerbo.local"
                  />
                </div>
                <div class="flex flex-col gap-1">
                  <label for="mqtt_port" class="classic-label px-1">Port</label>
                  <input
                    id="mqtt_port"
                    v-model.number="config.mqtt_port"
                    type="number"
                    class="classic-input w-full"
                  />
                </div>
                <div class="flex flex-col gap-1">
                  <label for="mqtt_login" class="classic-label px-1">Username</label>
                  <input
                    id="mqtt_login"
                    v-model="config.mqtt_login"
                    type="text"
                    class="classic-input w-full"
                    placeholder="Optional"
                  />
                </div>
                <div class="flex flex-col gap-1">
                  <label for="mqtt_password" class="classic-label px-1">Password</label>
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
                <label for="portal_id" class="classic-label px-1">VRM Portal ID</label>
                <input
                  id="portal_id"
                  v-model="config.portal_id"
                  type="text"
                  class="classic-input w-full"
                  placeholder="e.g. a1b2c3d4e5f6"
                />
                <p class="text-[10px] text-muted px-1 italic">Keep-alive for Cerbo GX.</p>
              </div>

              <div class="flex flex-col gap-2" role="radiogroup" aria-label="Interface Theme">
                <span class="classic-label px-1">Interface Theme</span>
                <div class="flex gap-1">
                  <UiButton
                    class="flex-1"
                    toggle
                    :active="config.color_scheme === 'dark'"
                    @click="config.color_scheme = 'dark'"
                  >
                    Dark
                  </UiButton>
                  <UiButton
                    class="flex-1"
                    toggle
                    :active="config.color_scheme === 'light'"
                    @click="config.color_scheme = 'light'"
                  >
                    Light
                  </UiButton>
                </div>
              </div>
            </div>

            <!-- Home Assistant Section -->
            <div v-if="activeTab === 'ha'" class="flex flex-col gap-4">
              <header class="border-b border-black/[0.06] dark:border-white/[0.07] pb-2">
                <h2 class="classic-section-title">Home Assistant</h2>
                <p class="text-[10px] text-muted mt-1">
                  HA API is for home devices (garage, laundry, EV, covers). Inverter control flags
                  always go to Cerbo MQTT, even when API is enabled.
                </p>
              </header>

              <div class="flex flex-col gap-3 p-3 classic-inset !rounded-lg !p-3">
                <div class="flex flex-col gap-1">
                  <label for="ha_url" class="classic-label px-1">Server URL</label>
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
                    <label for="ha_port" class="classic-label px-1">API Port</label>
                    <input
                      id="ha_port"
                      v-model.number="config.ha_port"
                      type="number"
                      class="classic-input w-full"
                      placeholder="8123"
                    />
                  </div>
                  <div class="flex flex-col gap-1">
                    <span class="classic-label px-1">Status</span>
                    <div
                      class="h-8 flex items-center px-2 rounded-lg border border-black/[0.08] dark:border-white/[0.1] bg-white dark:bg-[#1c1c1e] text-[10px] font-semibold"
                    >
                      <span :class="haDirectMonitoringEnabled ? 'text-green-500' : 'text-muted'">
                        API: {{ haDirectMonitoringEnabled ? 'Enabled' : 'Disabled' }}
                      </span>
                    </div>
                  </div>
                </div>
                <div class="flex flex-col gap-1">
                  <label for="ha_token" class="classic-label px-1">Access Token</label>
                  <input
                    id="ha_token"
                    v-model="config.ha_longlived_token"
                    type="password"
                    class="classic-input w-full"
                    placeholder="Token"
                  />
                </div>
                <div class="flex gap-2 mt-1">
                  <UiButton class="flex-1" :loading="testingHa" @click="testHaConnection">
                    {{ testingHa ? 'Testing...' : 'Test Connection' }}
                  </UiButton>
                  <UiButton
                    class="flex-1"
                    :disabled="discoveryLoading || !haDirectMonitoringEnabled"
                    :loading="discoveryLoading"
                    @click="handleFetchHaEntities"
                  >
                    Fetch Entities
                  </UiButton>
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
                <h3 class="classic-subsection-title">MQTT Routing</h3>
                <div class="grid grid-cols-2 gap-3">
                  <div class="flex flex-col gap-1">
                    <label for="mqtt_ha_host" class="classic-label px-1">HA MQTT Host</label>
                    <input
                      id="mqtt_ha_host"
                      v-model="config.mqtt_ha_host"
                      type="text"
                      class="classic-input w-full"
                    />
                  </div>
                  <div class="flex flex-col gap-1">
                    <label for="mqtt_ha_port" class="classic-label px-1">HA MQTT Port</label>
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
                    <label for="mqtt_ha_login" class="classic-label px-1">HA MQTT Username</label>
                    <input
                      id="mqtt_ha_login"
                      v-model="config.mqtt_ha_login"
                      type="text"
                      class="classic-input w-full"
                      placeholder="Optional"
                    />
                  </div>
                  <div class="flex flex-col gap-1">
                    <label for="mqtt_ha_password" class="classic-label px-1"
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
                <label
                  class="flex items-center gap-2 px-2 py-1.5 rounded-lg border border-black/[0.08] dark:border-white/[0.1] bg-white dark:bg-[#1c1c1e] cursor-pointer group hover:border-accent/40 transition-colors"
                >
                  <input
                    type="checkbox"
                    v-model="config.show_advanced_settings"
                    class="rounded border-slate-300 text-accent focus:ring-accent"
                  />
                  <span class="text-[11px] font-bold text-main">Advanced settings</span>
                </label>
                <div v-if="config.show_advanced_settings" class="flex flex-col gap-1">
                  <span class="classic-label px-1">Camera Monitoring</span>
                  <label
                    class="flex items-center gap-2 px-2 py-1.5 rounded-lg border border-black/[0.08] dark:border-white/[0.1] bg-white dark:bg-[#1c1c1e] cursor-pointer group hover:border-accent/40 transition-colors"
                  >
                    <input
                      type="checkbox"
                      v-model="config.camera_enabled"
                      class="rounded border-slate-300 text-accent focus:ring-accent"
                    />
                    <span class="text-[11px] font-bold text-main"
                      >Enable camera event detection</span
                    >
                  </label>
                </div>
                <div v-if="config.show_advanced_settings" class="flex flex-col gap-1">
                  <label for="camera_topic" class="classic-label px-1"
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
                  <p class="text-[10px] text-muted px-1 italic">
                    MQTT topic with wildcard for camera events on HA broker.
                  </p>
                </div>
              </div>

              <!-- Appliance Entities -->
              <div
                v-if="config.show_advanced_settings"
                class="flex flex-col gap-3 p-3 classic-inset !rounded-lg !p-3"
              >
                <h3 class="classic-subsection-title">Appliance Entities</h3>
                <div class="flex flex-col gap-1">
                  <label for="ha_dryer_entity" class="classic-label px-1">Dryer Entity</label>
                  <input
                    id="ha_dryer_entity"
                    v-model="config.ha_dryer_entity"
                    type="text"
                    class="classic-input w-full"
                    placeholder="sensor.dryer_remaining_time"
                  />
                  <p class="text-[10px] text-muted px-1 italic">
                    Remaining time sensor (e.g. 10:02). Section shows while the dryer is running.
                  </p>
                </div>
                <div class="flex flex-col gap-1">
                  <label for="ha_dryer_start_entity" class="classic-label px-1"
                    >Dryer Start Entity</label
                  >
                  <input
                    id="ha_dryer_start_entity"
                    v-model="config.ha_dryer_start_entity"
                    type="text"
                    class="classic-input w-full"
                    placeholder="button.dryer_remote_start"
                  />
                  <p class="text-[10px] text-muted px-1 italic">
                    Optional START button shown in the Dryer section.
                  </p>
                </div>
                <div class="flex flex-col gap-1">
                  <label for="ha_dryer_pause_entity" class="classic-label px-1"
                    >Dryer Pause Entity</label
                  >
                  <input
                    id="ha_dryer_pause_entity"
                    v-model="config.ha_dryer_pause_entity"
                    type="text"
                    class="classic-input w-full"
                    placeholder="button.dryer_pause"
                  />
                  <p class="text-[10px] text-muted px-1 italic">
                    Optional PAUSE button shown in the Dryer section.
                  </p>
                </div>
                <div class="flex flex-col gap-1">
                  <label for="ha_washer_entity" class="classic-label px-1">Washer Entity</label>
                  <input
                    id="ha_washer_entity"
                    v-model="config.ha_washer_entity"
                    type="text"
                    class="classic-input w-full"
                    placeholder="sensor.washer_remaining_time"
                  />
                  <p class="text-[10px] text-muted px-1 italic">
                    Remaining time sensor. Section shows while the washer is running.
                  </p>
                </div>
                <div class="flex flex-col gap-1">
                  <label for="ha_washer_start_entity" class="classic-label px-1"
                    >Washer Start Entity</label
                  >
                  <input
                    id="ha_washer_start_entity"
                    v-model="config.ha_washer_start_entity"
                    type="text"
                    class="classic-input w-full"
                    placeholder="button.washer_remote_start"
                  />
                  <p class="text-[10px] text-muted px-1 italic">
                    Optional START button shown in the Washer section.
                  </p>
                </div>
                <div class="flex flex-col gap-1">
                  <label for="ha_washer_pause_entity" class="classic-label px-1"
                    >Washer Pause Entity</label
                  >
                  <input
                    id="ha_washer_pause_entity"
                    v-model="config.ha_washer_pause_entity"
                    type="text"
                    class="classic-input w-full"
                    placeholder="button.washer_pause"
                  />
                  <p class="text-[10px] text-muted px-1 italic">
                    Optional PAUSE button shown in the Washer section.
                  </p>
                </div>
                <div class="flex flex-col gap-1">
                  <label for="ha_dishwasher_running_entity" class="classic-label px-1"
                    >Dishwasher Running Entity</label
                  >
                  <input
                    id="ha_dishwasher_running_entity"
                    v-model="config.ha_dishwasher_running_entity"
                    type="text"
                    class="classic-input w-full"
                    placeholder="binary_sensor.dishwasher_running"
                  />
                  <p class="text-[10px] text-muted px-1 italic">
                    Binary sensor that turns on while the dishwasher runs.
                  </p>
                </div>
                <div class="flex flex-col gap-1">
                  <label for="ha_dishwasher_duration_entity" class="classic-label px-1"
                    >Dishwasher Duration Entity</label
                  >
                  <input
                    id="ha_dishwasher_duration_entity"
                    v-model="config.ha_dishwasher_duration_entity"
                    type="text"
                    class="classic-input w-full"
                    placeholder="sensor.dishwasher_duration"
                  />
                  <p class="text-[10px] text-muted px-1 italic">
                    Runtime since midnight (e.g. 00:32:24), shown next to the section.
                  </p>
                </div>
              </div>

              <!-- Cerbo Water & EV (MQTT instance discovery) -->
              <div
                v-if="config.show_advanced_settings"
                class="flex flex-col gap-3 p-3 classic-inset !rounded-lg !p-3"
              >
                <h3 class="classic-subsection-title">Cerbo Water &amp; EV</h3>
                <p class="text-[10px] text-muted px-1">
                  Instances are auto-discovered from the Cerbo GX MQTT portal (dbus-pump / dbus-ev /
                  dbus-evcharger) after connect. Select which instance feeds the dashboard; values
                  still come from Cerbo, not Home Assistant entity IDs.
                </p>

                <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
                  <div class="flex flex-col gap-1.5">
                    <span class="classic-label px-1">Water tanks</span>
                    <p v-if="!discoveredTanks.length" class="text-[10px] text-muted px-1 italic">
                      Waiting for tank/+/… MQTT topics…
                    </p>
                    <button
                      v-for="item in discoveredTanks"
                      :key="'tank-' + item.instance"
                      type="button"
                      class="flex items-center justify-between gap-2 px-2 py-1.5 rounded-lg border text-left text-[11px] transition-colors"
                      :class="
                        config.water_tank_instance === item.instance
                          ? 'border-accent/60 bg-accent/10 text-main'
                          : 'border-black/[0.08] dark:border-white/[0.1] bg-white dark:bg-[#1c1c1e] text-main hover:border-accent/40'
                      "
                      @click="config.water_tank_instance = item.instance"
                    >
                      <span class="font-bold truncate">{{
                        item.name || 'Tank ' + item.instance
                      }}</span>
                      <span class="text-muted shrink-0">#{{ item.instance }}</span>
                    </button>
                  </div>

                  <div class="flex flex-col gap-1.5">
                    <span class="classic-label px-1">Pumps &amp; valves</span>
                    <p v-if="!discoveredPumps.length" class="text-[10px] text-muted px-1 italic">
                      Waiting for pump/+/… MQTT topics…
                    </p>
                    <div
                      v-for="item in discoveredPumps"
                      :key="'pump-' + item.instance"
                      class="flex flex-col gap-1 px-2 py-1.5 rounded-lg border border-black/[0.08] dark:border-white/[0.1] bg-white dark:bg-[#1c1c1e]"
                    >
                      <div class="flex items-center justify-between gap-2 text-[11px] text-main">
                        <span class="font-bold truncate">{{
                          item.name || 'Pump ' + item.instance
                        }}</span>
                        <span class="text-muted shrink-0">#{{ item.instance }}</span>
                      </div>
                      <div class="flex gap-2">
                        <button
                          type="button"
                          class="flex-1 py-0.5 rounded text-[10px] font-semibold border transition-colors"
                          :class="
                            config.water_pump_instance === item.instance
                              ? 'border-accent/60 bg-accent/10 text-main'
                              : 'border-black/[0.08] dark:border-white/[0.1] text-muted'
                          "
                          @click="config.water_pump_instance = item.instance"
                        >
                          Use as pump
                        </button>
                        <button
                          type="button"
                          class="flex-1 py-0.5 rounded text-[10px] font-semibold border transition-colors"
                          :class="
                            config.water_valve_instance === item.instance
                              ? 'border-accent/60 bg-accent/10 text-main'
                              : 'border-black/[0.08] dark:border-white/[0.1] text-muted'
                          "
                          @click="config.water_valve_instance = item.instance"
                        >
                          Use as valve
                        </button>
                      </div>
                    </div>
                  </div>

                  <div class="flex flex-col gap-1.5">
                    <span class="classic-label px-1">EV vehicles</span>
                    <p v-if="!discoveredEvs.length" class="text-[10px] text-muted px-1 italic">
                      Waiting for ev/+/… MQTT topics…
                    </p>
                    <button
                      v-for="item in discoveredEvs"
                      :key="'ev-' + item.instance"
                      type="button"
                      class="flex items-center justify-between gap-2 px-2 py-1.5 rounded-lg border text-left text-[11px] transition-colors"
                      :class="
                        config.ev_instance === item.instance
                          ? 'border-accent/60 bg-accent/10 text-main'
                          : 'border-black/[0.08] dark:border-white/[0.1] bg-white dark:bg-[#1c1c1e] text-main hover:border-accent/40'
                      "
                      @click="config.ev_instance = item.instance"
                    >
                      <span class="font-bold truncate">{{
                        item.name || 'EV ' + item.instance
                      }}</span>
                      <span class="text-muted shrink-0">#{{ item.instance }}</span>
                    </button>
                  </div>

                  <div class="flex flex-col gap-1.5">
                    <span class="classic-label px-1">EV chargers</span>
                    <p
                      v-if="!discoveredEvchargers.length"
                      class="text-[10px] text-muted px-1 italic"
                    >
                      Waiting for evcharger/+/… MQTT topics…
                    </p>
                    <button
                      v-for="item in discoveredEvchargers"
                      :key="'evc-' + item.instance"
                      type="button"
                      class="flex items-center justify-between gap-2 px-2 py-1.5 rounded-lg border text-left text-[11px] transition-colors"
                      :class="
                        config.evcharger_instance === item.instance
                          ? 'border-accent/60 bg-accent/10 text-main'
                          : 'border-black/[0.08] dark:border-white/[0.1] bg-white dark:bg-[#1c1c1e] text-main hover:border-accent/40'
                      "
                      @click="config.evcharger_instance = item.instance"
                    >
                      <span class="font-bold truncate">{{
                        item.name || 'Charger ' + item.instance
                      }}</span>
                      <span class="text-muted shrink-0">#{{ item.instance }}</span>
                    </button>
                  </div>
                </div>

                <details class="mt-1">
                  <summary
                    class="text-[10px] text-muted px-1 cursor-pointer select-none hover:text-main"
                  >
                    Manual instance override
                  </summary>
                  <div class="flex flex-col gap-2 mt-2 px-1">
                    <div class="flex flex-wrap items-center gap-3">
                      <label for="water_tank_instance" class="classic-label">Tank</label>
                      <input
                        id="water_tank_instance"
                        v-model.number="config.water_tank_instance"
                        type="number"
                        min="0"
                        class="classic-input w-20"
                      />
                      <label for="water_pump_instance" class="classic-label">Pump</label>
                      <input
                        id="water_pump_instance"
                        v-model.number="config.water_pump_instance"
                        type="number"
                        min="1"
                        class="classic-input w-20"
                      />
                      <label for="water_valve_instance" class="classic-label">Valve</label>
                      <input
                        id="water_valve_instance"
                        v-model.number="config.water_valve_instance"
                        type="number"
                        min="1"
                        class="classic-input w-20"
                      />
                    </div>
                    <div class="flex flex-wrap items-center gap-3">
                      <label for="ev_instance" class="classic-label">EV vehicle</label>
                      <input
                        id="ev_instance"
                        v-model.number="config.ev_instance"
                        type="number"
                        min="1"
                        class="classic-input w-20"
                      />
                      <label for="evcharger_instance" class="classic-label">EV charger</label>
                      <input
                        id="evcharger_instance"
                        v-model.number="config.evcharger_instance"
                        type="number"
                        min="1"
                        class="classic-input w-20"
                      />
                    </div>
                    <p class="text-[10px] text-muted italic">
                      Optional fallback when discovery is empty. Saved values are kept when they
                      still appear in the discovered list; otherwise the first found instance is
                      used for live tiles.
                    </p>
                  </div>
                </details>
              </div>
            </div>

            <!-- Sections Visibility -->
            <div v-if="activeTab === 'sections'" class="flex flex-col gap-4">
              <header class="border-b border-black/[0.06] dark:border-white/[0.07] pb-2">
                <h2 class="classic-section-title">Section Visibility</h2>
              </header>

              <!-- Group 1: Inverter & Solar -->
              <div class="flex flex-col gap-2 p-3 classic-inset !rounded-lg !p-3">
                <h3 class="classic-subsection-title">Inverter & Solar</h3>
                <label
                  class="flex items-center gap-2 px-2 py-1.5 rounded-lg border border-black/[0.08] dark:border-white/[0.1] bg-white dark:bg-[#1c1c1e] cursor-pointer group hover:border-accent/40 transition-colors"
                >
                  <input
                    type="checkbox"
                    v-model="config.show_batteries"
                    class="rounded border-slate-300 text-accent focus:ring-accent"
                  />
                  <span class="text-[11px] font-bold text-main">Batteries</span>
                </label>
                <label
                  class="flex items-center gap-2 px-2 py-1.5 rounded-lg border border-black/[0.08] dark:border-white/[0.1] bg-white dark:bg-[#1c1c1e] cursor-pointer group hover:border-accent/40 transition-colors"
                >
                  <input
                    type="checkbox"
                    v-model="config.show_solar_production"
                    class="rounded border-slate-300 text-accent focus:ring-accent"
                  />
                  <span class="text-[11px] font-bold text-main">Solar Production</span>
                </label>
              </div>

              <!-- Group 2: Energy Stats -->
              <div class="flex flex-col gap-2 p-3 classic-inset !rounded-lg !p-3">
                <h3 class="classic-subsection-title">Energy Stats</h3>
                <label
                  class="flex items-center gap-2 px-2 py-1.5 rounded-lg border border-black/[0.08] dark:border-white/[0.1] bg-white dark:bg-[#1c1c1e] cursor-pointer group hover:border-accent/40 transition-colors"
                >
                  <input
                    type="checkbox"
                    v-model="config.show_active_loads"
                    class="rounded border-slate-300 text-accent focus:ring-accent"
                  />
                  <span class="text-[11px] font-bold text-main">Active Loads</span>
                </label>
                <label
                  class="flex items-center gap-2 px-2 py-1.5 rounded-lg border border-black/[0.08] dark:border-white/[0.1] bg-white dark:bg-[#1c1c1e] cursor-pointer group hover:border-accent/40 transition-colors"
                >
                  <input
                    type="checkbox"
                    v-model="config.show_daily_stats"
                    class="rounded border-slate-300 text-accent focus:ring-accent"
                  />
                  <span class="text-[11px] font-bold text-main">Daily Stats</span>
                </label>
              </div>

              <!-- Group 3: Home Area -->
              <div class="flex flex-col gap-2 p-3 classic-inset !rounded-lg !p-3">
                <h3 class="classic-subsection-title">Home Area</h3>
                <label
                  class="flex items-center gap-2 px-2 py-1.5 rounded-lg border border-black/[0.08] dark:border-white/[0.1] bg-white dark:bg-[#1c1c1e] cursor-pointer group hover:border-accent/40 transition-colors"
                >
                  <input
                    type="checkbox"
                    v-model="config.show_ev"
                    class="rounded border-slate-300 text-accent focus:ring-accent"
                  />
                  <span class="text-[11px] font-bold text-main">EV</span>
                </label>
                <label
                  class="flex items-center gap-2 px-2 py-1.5 rounded-lg border border-black/[0.08] dark:border-white/[0.1] bg-white dark:bg-[#1c1c1e] cursor-pointer group hover:border-accent/40 transition-colors"
                >
                  <input
                    type="checkbox"
                    v-model="config.show_washer"
                    class="rounded border-slate-300 text-accent focus:ring-accent"
                  />
                  <span class="text-[11px] font-bold text-main">Washer</span>
                </label>
                <label
                  class="flex items-center gap-2 px-2 py-1.5 rounded-lg border border-black/[0.08] dark:border-white/[0.1] bg-white dark:bg-[#1c1c1e] cursor-pointer group hover:border-accent/40 transition-colors"
                >
                  <input
                    type="checkbox"
                    v-model="config.show_dryer"
                    class="rounded border-slate-300 text-accent focus:ring-accent"
                  />
                  <span class="text-[11px] font-bold text-main">Dryer</span>
                </label>
                <label
                  class="flex items-center gap-2 px-2 py-1.5 rounded-lg border border-black/[0.08] dark:border-white/[0.1] bg-white dark:bg-[#1c1c1e] cursor-pointer group hover:border-accent/40 transition-colors"
                >
                  <input
                    type="checkbox"
                    v-model="config.show_dishwasher"
                    class="rounded border-slate-300 text-accent focus:ring-accent"
                  />
                  <span class="text-[11px] font-bold text-main">Dishwasher</span>
                </label>
                <label
                  class="flex items-center gap-2 px-2 py-1.5 rounded-lg border border-black/[0.08] dark:border-white/[0.1] bg-white dark:bg-[#1c1c1e] cursor-pointer group hover:border-accent/40 transition-colors"
                >
                  <input
                    type="checkbox"
                    v-model="config.show_home_section"
                    class="rounded border-slate-300 text-accent focus:ring-accent"
                  />
                  <span class="text-[11px] font-bold text-main">Home Buttons</span>
                </label>
                <label
                  class="flex items-center gap-2 px-2 py-1.5 rounded-lg border border-black/[0.08] dark:border-white/[0.1] bg-white dark:bg-[#1c1c1e] cursor-pointer group hover:border-accent/40 transition-colors"
                >
                  <input
                    type="checkbox"
                    v-model="config.show_header_toggles"
                    class="rounded border-slate-300 text-accent focus:ring-accent"
                  />
                  <span class="text-[11px] font-bold text-main">{{
                    $t('config.headerToggles')
                  }}</span>
                </label>
                <label
                  class="flex items-center gap-2 px-2 py-1.5 rounded-lg border border-black/[0.08] dark:border-white/[0.1] bg-white dark:bg-[#1c1c1e] cursor-pointer group hover:border-accent/40 transition-colors"
                >
                  <input
                    type="checkbox"
                    v-model="config.show_ha_sensors"
                    class="rounded border-slate-300 text-accent focus:ring-accent"
                  />
                  <span class="text-[11px] font-bold text-main">{{ $t('config.sensors') }}</span>
                </label>
                <label
                  class="flex items-center gap-2 px-2 py-1.5 rounded-lg border border-black/[0.08] dark:border-white/[0.1] bg-white dark:bg-[#1c1c1e] cursor-pointer group hover:border-accent/40 transition-colors"
                >
                  <input
                    type="checkbox"
                    v-model="config.show_ha_numbers"
                    class="rounded border-slate-300 text-accent focus:ring-accent"
                  />
                  <span class="text-[11px] font-bold text-main">{{ $t('config.numbers') }}</span>
                </label>
                <label
                  class="flex items-center gap-2 px-2 py-1.5 rounded-lg border border-black/[0.08] dark:border-white/[0.1] bg-white dark:bg-[#1c1c1e] cursor-pointer group hover:border-accent/40 transition-colors"
                >
                  <input
                    type="checkbox"
                    v-model="config.show_ha_covers"
                    class="rounded border-slate-300 text-accent focus:ring-accent"
                  />
                  <span class="text-[11px] font-bold text-main">{{ $t('config.covers') }}</span>
                </label>
                <label
                  class="flex items-center gap-2 px-2 py-1.5 rounded-lg border border-black/[0.08] dark:border-white/[0.1] bg-white dark:bg-[#1c1c1e] cursor-pointer group hover:border-accent/40 transition-colors"
                >
                  <input
                    type="checkbox"
                    v-model="config.show_ha_media"
                    class="rounded border-slate-300 text-accent focus:ring-accent"
                  />
                  <span class="text-[11px] font-bold text-main">{{
                    $t('config.mediaPlayers')
                  }}</span>
                </label>
                <label
                  class="flex items-center gap-2 px-2 py-1.5 rounded-lg border border-black/[0.08] dark:border-white/[0.1] bg-white dark:bg-[#1c1c1e] cursor-pointer group hover:border-accent/40 transition-colors"
                >
                  <input
                    type="checkbox"
                    v-model="config.show_ha_scenes"
                    class="rounded border-slate-300 text-accent focus:ring-accent"
                  />
                  <span class="text-[11px] font-bold text-main">{{ $t('config.scenes') }}</span>
                </label>
                <label
                  class="flex items-center gap-2 px-2 py-1.5 rounded-lg border border-black/[0.08] dark:border-white/[0.1] bg-white dark:bg-[#1c1c1e] cursor-pointer group hover:border-accent/40 transition-colors"
                >
                  <input
                    type="checkbox"
                    v-model="config.show_ha_weather"
                    class="rounded border-slate-300 text-accent focus:ring-accent"
                  />
                  <span class="text-[11px] font-bold text-main">{{ $t('config.weather') }}</span>
                </label>
                <label
                  class="flex items-center gap-2 px-2 py-1.5 rounded-lg border border-black/[0.08] dark:border-white/[0.1] bg-white dark:bg-[#1c1c1e] cursor-pointer group hover:border-accent/40 transition-colors"
                >
                  <input
                    type="checkbox"
                    v-model="config.show_console"
                    class="rounded border-slate-300 text-accent focus:ring-accent"
                  />
                  <span class="text-[11px] font-bold text-main">Console</span>
                </label>

                <!-- App Settings -->
                <div class="flex flex-col gap-2 p-3 classic-inset !rounded-lg !p-3">
                  <h3 class="classic-subsection-title">App Settings</h3>
                  <label
                    class="flex items-center gap-2 px-2 py-1.5 rounded-lg border border-black/[0.08] dark:border-white/[0.1] bg-white dark:bg-[#1c1c1e] cursor-pointer group hover:border-accent/40 transition-colors"
                  >
                    <input
                      type="checkbox"
                      :checked="config.auto_start"
                      @change="config.auto_start = ($event.target as HTMLInputElement).checked"
                      class="rounded border-slate-300 text-accent focus:ring-accent"
                    />
                    <span class="text-[11px] font-bold text-main"> Launch at system startup </span>
                  </label>
                </div>

                <!-- Authentication -->
                <div class="flex flex-col gap-2 p-3 classic-inset !rounded-lg !p-3">
                  <h3 class="classic-subsection-title">Authentication</h3>
                  <label
                    class="flex items-center gap-2 px-2 py-1.5 rounded-lg border border-black/[0.08] dark:border-white/[0.1] bg-white dark:bg-[#1c1c1e] cursor-pointer group hover:border-accent/40 transition-colors"
                  >
                    <input
                      type="checkbox"
                      :checked="config.auth_enabled"
                      @change="config.auth_enabled = ($event.target as HTMLInputElement).checked"
                      class="rounded border-slate-300 text-accent focus:ring-accent"
                    />
                    <span class="text-[11px] font-bold text-main"> Enable authentication </span>
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
                        class="rounded border border-slate-300 dark:border-slate-600 bg-white dark:bg-[#1a1a1a] px-2 py-1 text-[11px] text-main"
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
                        class="rounded border border-slate-300 dark:border-slate-600 bg-white dark:bg-[#1a1a1a] px-2 py-1 text-[11px] text-main"
                      />
                    </div>
                    <label
                      class="flex items-center gap-2 px-2 py-1.5 rounded-lg border border-black/[0.08] dark:border-white/[0.1] bg-white dark:bg-[#1c1c1e] cursor-pointer group hover:border-accent/40 transition-colors"
                    >
                      <input
                        type="checkbox"
                        :checked="config.auth_biometric"
                        @change="
                          config.auth_biometric = ($event.target as HTMLInputElement).checked
                        "
                        class="rounded border-slate-300 text-accent focus:ring-accent"
                      />
                      <span class="text-[11px] font-bold text-main">
                        Allow biometric authentication (Touch ID / Windows Hello)
                      </span>
                    </label>
                  </div>
                </div>
              </div>
            </div>

            <!-- Backup Section -->
            <div v-if="activeTab === 'backup'" class="flex flex-col gap-4">
              <header class="border-b border-black/[0.06] dark:border-white/[0.07] pb-2">
                <h2 class="classic-section-title">Backup</h2>
              </header>

              <div class="flex flex-col gap-3 p-3 classic-inset !rounded-lg !p-3">
                <p class="text-[10px] text-muted px-1 italic">
                  Export the current configuration to a JSON file or import one previously saved.
                </p>
                <UiButton class="flex-1 w-full" :loading="backupBusy" @click="handleBackup">
                  <Download v-if="!backupBusy" :size="12" />
                  Save Configuration
                </UiButton>
                <UiButton class="flex-1 w-full" :disabled="backupBusy" @click="handleRestore">
                  <Upload :size="12" />
                  Load Configuration
                </UiButton>
              </div>
            </div>

            <!-- Entities Section -->
            <div v-if="activeTab === 'entities'" class="flex flex-col gap-6">
              <header class="border-b border-black/[0.06] dark:border-white/[0.07] pb-2">
                <h2 class="classic-section-title">UI Controls</h2>
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
            <div
              v-if="discoveryLoading"
              class="flex flex-col items-center justify-center py-10 gap-2"
            >
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
              <div
                v-for="e in filteredDiscoveredEntities"
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
                  <div class="text-[9px] text-muted font-mono">
                    {{ e.entity_id }}
                  </div>
                </div>
                <div v-if="selectedDiscovery.includes(e.entity_id)" class="text-accent">
                  <Check :size="12" />
                </div>
              </div>
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
                Header Toggles
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

      <!-- Toast Notification -->
      <div
        v-if="message"
        class="fixed bottom-4 left-1/2 -translate-x-1/2 z-[60] px-4 py-1.5 rounded-full shadow-lg text-[10px] font-bold border animate-in slide-in-from-bottom duration-200"
        :class="
          messageType === 'error'
            ? 'bg-red-500 border-red-600 text-white'
            : 'bg-green-500 border-green-600 text-white'
        "
      >
        {{ message }}
      </div>
    </div>
  </ErrorBoundary>
</template>

<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import type { InverterState } from './composables/useInverterState'
import { useI18n } from 'vue-i18n'
import ErrorBoundary from './components/ErrorBoundary.vue'
import UiButton from './components/UiButton.vue'
import { logger } from './logger'

const { t: $t } = useI18n()

import {
  Archive,
  Check,
  Download,
  Eye,
  Home,
  Layout,
  Loader2,
  RotateCcw,
  Save,
  Settings,
  Upload,
  Wifi,
  X,
} from '@lucide/vue'
import HaEntitiesEditor from './components/HaEntitiesEditor.vue'
import HeaderTogglesEditor from './components/HeaderTogglesEditor.vue'
import { useConfigForm } from './composables/useConfigForm'
import { useHAEntityManager } from './composables/useHAEntityManager'

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
  discoverySearch,
  filteredDiscoveredEntities,
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

type DiscoveredInst = {
  instance: number
  kind: string
  name?: string | null
}
const discoveredWaterEv = ref<DiscoveredInst[]>([])
const discoveredTanks = computed(() => discoveredWaterEv.value.filter((i) => i.kind === 'tank'))
const discoveredPumps = computed(() => discoveredWaterEv.value.filter((i) => i.kind === 'pump'))
const discoveredEvs = computed(() => discoveredWaterEv.value.filter((i) => i.kind === 'ev'))
const discoveredEvchargers = computed(() =>
  discoveredWaterEv.value.filter((i) => i.kind === 'evcharger')
)

function ingestDiscovered(list: DiscoveredInst[] | null | undefined) {
  if (!list || !list.length) return
  discoveredWaterEv.value = list
  // Auto-pick sensible defaults when saved config is missing from discovery.
  const tanks = list.filter((i) => i.kind === 'tank')
  const pumps = list.filter((i) => i.kind === 'pump')
  const evs = list.filter((i) => i.kind === 'ev')
  const chargers = list.filter((i) => i.kind === 'evcharger')
  const pick = (current: number | undefined, items: DiscoveredInst[]) => {
    if (!items.length) return current
    if (current != null && items.some((i) => i.instance === current)) return current
    return items[0].instance
  }
  config.water_tank_instance = pick(config.water_tank_instance, tanks)
  config.water_pump_instance = pick(config.water_pump_instance, pumps)
  // Valve: keep saved if present; else first pump that is not the active pump
  if (pumps.length) {
    const valve = config.water_valve_instance
    if (valve == null || !pumps.some((i) => i.instance === valve)) {
      const alt = pumps.find((i) => i.instance !== config.water_pump_instance)
      if (alt) config.water_valve_instance = alt.instance
    }
  }
  config.ev_instance = pick(config.ev_instance, evs)
  config.evcharger_instance = pick(config.evcharger_instance, chargers)
}

let unlistenMqttState: UnlistenFn | null = null

const sections = [
  { id: 'mqtt', label: 'MQTT Broker', icon: Wifi },
  { id: 'ha', label: 'Home Assistant', icon: Home },
  { id: 'entities', label: 'UI Controls', icon: Layout },
  { id: 'sections', label: 'Sections', icon: Eye },
  { id: 'backup', label: 'Backup', icon: Archive },
]

const testingHa = ref(false)
const haTestResult = ref('')
const haTestSuccess = ref(false)
const backupBusy = ref(false)

const haDirectMonitoringEnabled = computed(() => {
  return !!(
    config.ha_url &&
    config.ha_longlived_token &&
    config.ha_url.trim() &&
    config.ha_longlived_token.trim()
  )
})

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

async function handleBackup() {
  if (backupBusy.value) return
  backupBusy.value = true
  try {
    const done = await invoke<boolean>('backup_config')
    message.value = done ? 'Configuration saved' : 'Backup cancelled'
    messageType.value = done ? 'success' : 'info'
  } catch (e) {
    message.value = `Backup failed: ${e?.toString() || e}`
    messageType.value = 'error'
  } finally {
    backupBusy.value = false
    setTimeout(clearMessage, 3000)
  }
}

async function handleRestore() {
  if (backupBusy.value) return
  backupBusy.value = true
  try {
    const done = await invoke<boolean>('restore_config')
    if (done) {
      const cfg = await loadConfig()
      loadFromConfig(cfg)
      applyTheme(cfg.color_scheme)
      await emit('config-saved', { color_scheme: cfg.color_scheme })
      message.value = 'Configuration loaded'
      messageType.value = 'success'
    } else {
      message.value = 'Load cancelled'
      messageType.value = 'info'
    }
  } catch (e) {
    message.value = `Restore failed: ${e?.toString() || e}`
    messageType.value = 'error'
  } finally {
    backupBusy.value = false
    setTimeout(clearMessage, 3000)
  }
}

async function handleClose() {
  try {
    const win = getCurrentWindow()
    await win.close()
  } catch (e) {
    logger.warn('Frontend close failed, trying backend:', e)
    try {
      await invoke('close_config_window')
    } catch (err) {
      logger.error('Close failed:', err)
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
  logger.log('Applying theme to Config window:', scheme, isDark)
  document.documentElement.classList.toggle('dark', isDark)
  document.body.classList.toggle('dark', isDark)

  // Force background to prevent system-level dark mode overrides if any
  if (isDark) {
    document.documentElement.style.backgroundColor = '#0c0c0e'
    document.body.style.backgroundColor = '#0c0c0e'
  } else {
    document.documentElement.style.backgroundColor = '#f4f4f6'
    document.body.style.backgroundColor = '#f4f4f6'
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
    try {
      const st = await invoke<InverterState>('get_state')
      ingestDiscovered(st.discovered_water_ev ?? undefined)
    } catch (e) {
      logger.warn('get_state for water/EV discovery failed:', e)
    }
    unlistenMqttState = await listen<InverterState>('mqtt-state-update', (event) => {
      ingestDiscovered(event.payload?.discovered_water_ev ?? undefined)
    })
  } catch (err) {
    logger.error('Config init failed:', err)
  }
})

onUnmounted(() => {
  globalThis.removeEventListener('keydown', handleKeyDown)
  if (unlistenMqttState) {
    unlistenMqttState()
    unlistenMqttState = null
  }
})
</script>
