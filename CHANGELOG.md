# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added
- Manual pump/valve buttons in the Water card, routed through dbus-pump's
  writable `/Mode` (0 auto, 1 always-on, 2 always-off) via the GX MQTT-API
  (`W/<portal>/pump/<n>/Mode`) - still no direct Home Assistant control.
  Opening the city valve asks for confirmation; an AUTO chip appears while a
  device is under manual override to hand it back to dbus-pump automation.
- dbus-pump fix required on the GX: its `/Mode` onchange handler must return
  true, otherwise vedbus rejects the write and the mode silently stays `auto`
  (deployed as dbus-pump main).

### Changed
- EV section now sources data from Cerbo MQTT (dbus-ev / dbus-evcharger) instead of
  Home Assistant. Subscribes to `N/<portal>/ev/<instance>/Soc` (%),
  `N/<portal>/ev/<instance>/Ac/Power` (W), and
  `N/<portal>/evcharger/<instance>/Ac/Power` (W).
  New config fields: `evcharger_instance` (default 40) and `ev_instance` (default 22).
  The `ha_ev_soc_entity`, `ha_ev_charging_entity`, and `ha_ev_clamp_entity` config
  fields are removed. EV section visibility no longer requires HA direct API — it shows
  when Cerbo MQTT is connected and at least one EV metric is live.

## [2.5.0] - 2026-08-24

### Added
- Portal ID auto-discovery: subscribes to the retained `inverter/portal`
  topic published by inverter-control and arms water/alarm subscriptions and
  the GX keepalive automatically - `portal_id` is now optional in app config.

### Changed
- Water section now uses **only** Cerbo GX MQTT (dbus-pump): the Home Assistant
  entity fallback (`ha_water_level_entity` / `ha_valve_switch_entity` /
  `ha_pump_switch_entity`) is removed, along with its config fields
- Water card shows level in % plus pump/valve status badges; toggle buttons
  removed — pump/valve automation lives in dbus-pump

## [2.4.4] - 2026-08-23

### Added
- Solar forecast display (#252) and forecast brackets in DailyStats (#251)
- Yesterday solar production (#241)
- Washer/dryer START/PAUSE buttons from HA button entities (#247)
- Persistent notification banner + Victron alarm watcher (#244)

### Fixed
- Tray icon keeps updating after poisoned lock, panic, or sleep/wake (#255)
- Solar breakdown now adds up to headline total (#254)
- Existing config/about windows brought to front on reopen (#253)

### Security
- Bump glob override to 11.1.0 (CVE-2025-64756) (#250)

## [2.4.3] - unreleased

### Fixed
- Various fixes and dependency updates

## [2.2.2] - 2026-07-20

### Fixed

- CI build failure: remove empty Apple code signing env vars causing `SecKeychainItemImport` error
