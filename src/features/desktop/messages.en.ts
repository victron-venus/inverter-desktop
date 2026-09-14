import pluginManagerMessages from './plugins/locales/en.json'

export default {
  plugins: {
    manager: pluginManagerMessages,
    unavailable: 'Temporarily unavailable',
    actionFailed: 'Unable to complete the action. Try again.',
  },
  status: {
    haStale: 'Home Assistant offline — last known values',
    ha: 'HA',
    haMqtt: 'HA MQTT',
    tipHa: 'Home Assistant WebSocket connection',
    tipHaMqtt: 'HA MQTT broker for Kerberos camera motion events',
  },
  sections: {
    start: 'START',
    pause: 'PAUSE',
    sensors: 'Sensors',
    numbers: 'Numbers',
    covers: 'Covers',
    media: 'Media',
    scenes: 'Scenes',
    weather: 'Weather',
    dishwasher: 'Dishwasher',
    washer: 'Washer',
    dryer: 'Dryer',
    running: 'Running',
  },
  actions: {
    cameraMotion: 'Camera motion',
  },
  config: {
    homeAssistant: 'Home Assistant',
    weather: 'Weather',
    sensors: 'Sensors',
    numbers: 'Numbers',
    covers: 'Covers',
    mediaPlayers: 'Media Players',
    scenes: 'Scenes',
    homeButtons: 'Home Buttons',
    headerControlTarget: 'Inverter flag / HA entity',
    headerControlsHelp:
      'Inverter controls are provided by inverter-control through Cerbo MQTT and work without Home Assistant. You can also add custom Home Assistant entities.',
    invalidHeaderControlTarget: 'Use an inverter flag or a Home Assistant entity (domain.entity).',
    haControlsHelp:
      'Home Assistant is optional and supplies home device controls. Inverter controls are provided by inverter-control through Cerbo MQTT; Home Assistant can expose them as switches in its own ecosystem.',
    haSettings: 'Home Assistant',
    fetchEntities: 'Fetch Entities',
    cameraEventsTitle: 'Camera Events (HA MQTT)',
    cameraEventsHelp:
      'Separate MQTT connection to Home Assistant for Kerberos, Frigate, and/or Ring-MQTT camera motion. Cerbo MQTT stays on the Inverter tab.',
    haMqttHost: 'HA MQTT Host',
    haMqttHostPlaceholder: 'homeassistant.local',
    haMqttPort: 'HA MQTT Port',
    haMqttUsername: 'HA MQTT Username',
    haMqttPassword: 'HA MQTT Password',
    cameraMonitoring: 'Camera Monitoring',
    cameraEnabled: 'Enable camera motion events',
    cameraTopic: 'Camera Event Topic(s)',
    cameraTopicPlaceholder:
      'kerberos/desktop/events;frigate/events;ring/+/camera/+/motion/state;ring/+/camera/+/ding/state',
    cameraTopicHelp:
      'Semicolon-separated MQTT topics/patterns. Kerberos: kerberos/desktop/events (JSON agent_name, video_url). Frigate: frigate/events (needs Frigate base URL below). Ring-MQTT: ring/+/camera/+/motion/state and ring/+/camera/+/ding/state (needs snapshot URL template below; RTSP is not played in-app).',
    frigateBaseUrl: 'Frigate Base URL',
    frigateBaseUrlPlaceholder: 'https://192.168.151.21:5005',
    frigateBaseUrlHelp:
      'HTTP base for Frigate clip URLs ({base}/api/events/{id}/clip.mp4). Required when frigate/events is subscribed; leave empty to skip Frigate clips. Example: Synology host :5005→5000.',
    ringSnapshotUrlTemplate: 'Ring Snapshot URL Template',
    ringSnapshotUrlTemplatePlaceholder:
      'https://homeassistant.example.com/api/camera_proxy/camera.front_door_snapshot',
    ringSnapshotUrlTemplateHelp:
      'HTTP(S) URL opened on Ring motion/ding ON. Placeholders: {device_id}, {location_id}, {event}. Use HA camera_proxy (token from HA settings) or any LAN snapshot URL. Leave empty for notify-only. RTSP is not supported by the camera window — see docs/ring-mqtt.md.',
    cameraMotionDetected: '{agent} camera motion detected',
  },
}
