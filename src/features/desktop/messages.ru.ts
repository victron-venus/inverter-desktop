export default {
  plugins: {
    unavailable: 'Временно недоступно',
    actionFailed: 'Не удалось выполнить действие. Попробуйте ещё раз.',
  },
  actions: {
    cameraMotion: 'Движение камер',
  },
  status: {
    haStale: 'Home Assistant недоступен — последние известные значения',
    ha: 'HA',
    haMqtt: 'HA MQTT',
    tipHa: 'Подключение к Home Assistant WebSocket',
    tipHaMqtt: 'MQTT-брокер HA для событий движения камер Kerberos',
  },
  config: {
    homeAssistant: 'Home Assistant',
    weather: 'Погода',
    sensors: 'Датчики',
    numbers: 'Числа',
    covers: 'Шторы / крышки',
    mediaPlayers: 'Медиаплееры',
    scenes: 'Сцены',
    homeButtons: 'Кнопки Home',
    headerControlTarget: 'Флаг инвертора / сущность HA',
    headerControlsHelp:
      'Управление инвертором предоставляет inverter-control через Cerbo MQTT. Home Assistant для него не нужен. Можно также добавить свои сущности Home Assistant.',
    invalidHeaderControlTarget:
      'Укажите флаг инвертора или сущность Home Assistant (domain.entity).',
    haControlsHelp:
      'Home Assistant подключается по желанию и управляет домашними устройствами. Управление инвертором предоставляет inverter-control через Cerbo MQTT; Home Assistant может представить его как переключатели в своей экосистеме.',
    haSettings: 'Home Assistant',
    fetchEntities: 'Загрузить сущности',
    cameraEventsTitle: 'События камер (HA MQTT)',
    cameraEventsHelp:
      'Отдельное MQTT-подключение к Home Assistant для движения Kerberos, Frigate и/или Ring-MQTT. Cerbo MQTT остаётся на вкладке Inverter.',
    haMqttHost: 'Хост HA MQTT',
    haMqttHostPlaceholder: 'homeassistant.local',
    haMqttPort: 'Порт HA MQTT',
    haMqttUsername: 'Имя пользователя HA MQTT',
    haMqttPassword: 'Пароль HA MQTT',
    cameraMonitoring: 'Мониторинг камер',
    cameraEnabled: 'Включить события движения камер',
    cameraTopic: 'Топик(и) событий камеры',
    cameraTopicPlaceholder:
      'kerberos/desktop/events;frigate/events;ring/+/camera/+/motion/state;ring/+/camera/+/ding/state',
    cameraTopicHelp:
      'Топики/шаблоны MQTT через точку с запятой. Kerberos: kerberos/desktop/events (JSON agent_name, video_url). Frigate: frigate/events (нужен базовый URL Frigate ниже). Ring-MQTT: ring/+/camera/+/motion/state и ring/+/camera/+/ding/state (нужен шаблон snapshot ниже; RTSP в окне не воспроизводится).',
    frigateBaseUrl: 'Базовый URL Frigate',
    frigateBaseUrlPlaceholder: 'https://192.168.151.21:5005',
    frigateBaseUrlHelp:
      'HTTP-база для клипов Frigate ({base}/api/events/{id}/clip.mp4). Обязателен при подписке на frigate/events; пустое значение — клипы Frigate пропускаются. Пример: Synology :5005→5000.',
    ringSnapshotUrlTemplate: 'Шаблон URL снимка Ring',
    ringSnapshotUrlTemplatePlaceholder:
      'https://homeassistant.example.com/api/camera_proxy/camera.front_door_snapshot',
    ringSnapshotUrlTemplateHelp:
      'HTTP(S) URL при Ring motion/ding ON. Плейсхолдеры: {device_id}, {location_id}, {event}. Удобно camera_proxy HA (токен из настроек HA). Пусто — только уведомление. RTSP окном не поддерживается — см. docs/ring-mqtt.md.',
    cameraMotionDetected: '{agent} — обнаружено движение камеры',
  },
}
