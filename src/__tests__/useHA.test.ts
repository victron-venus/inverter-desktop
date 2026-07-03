import { describe, expect, it, vi } from 'vitest'
import type { HaCoverDisplay, HaSensorDisplay, HaWeatherDisplay } from '../types/ha'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => vi.fn()(...args),
}))

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
  emit: vi.fn(),
}))

vi.mock('vue-i18n', () => ({
  useI18n: () => ({ t: (key: string) => key }),
}))

const SENSOR_DOMAINS = ['sensor', 'binary_sensor'] as const

function filterSensors(
  entityStates: Record<string, string>,
  entityAttributes: Record<string, Record<string, unknown>>
): HaSensorDisplay[] {
  const result: HaSensorDisplay[] = []
  for (const [entityId, state] of Object.entries(entityStates)) {
    const domain = entityId.split('.')[0]
    if (!(SENSOR_DOMAINS as readonly string[]).includes(domain)) continue
    if (state === 'unavailable' || state === 'unknown') continue
    const attrs = entityAttributes[entityId] || {}
    const name = (attrs.friendly_name as string) || entityId
    const unit = (attrs.unit_of_measurement as string) || ''
    result.push({ entity_id: entityId, name, state, unit })
  }
  return result
}

function filterCovers(
  entityStates: Record<string, string>,
  entityAttributes: Record<string, Record<string, unknown>>
): HaCoverDisplay[] {
  const result: HaCoverDisplay[] = []
  for (const [entityId, attrs] of Object.entries(entityAttributes)) {
    if (!entityId.startsWith('cover.')) continue
    const state = entityStates[entityId]
    if (state === 'unavailable' || state === 'unknown') continue
    const name = (attrs.friendly_name as string) || entityId
    const position = (attrs.current_position as number) ?? 0
    result.push({ entity_id: entityId, name, position })
  }
  return result
}

function filterWeather(
  entityStates: Record<string, string>,
  entityAttributes: Record<string, Record<string, unknown>>
): HaWeatherDisplay | null {
  for (const [entityId, attrs] of Object.entries(entityAttributes)) {
    if (!entityId.startsWith('weather.')) continue
    const state = entityStates[entityId]
    if (state === 'unavailable' || state === 'unknown') continue
    const name = (attrs.friendly_name as string) || 'Weather'
    const temperature = (attrs.temperature as number) ?? null
    const unit = (attrs.temperature_unit as string) || '°C'
    const forecast = (attrs.forecast as Array<Record<string, unknown>>) ?? []
    return { entity_id: entityId, name, state, temperature, unit, forecast }
  }
  return null
}

function computeToggleStates(
  headerToggles: Array<{ id: string; entity: string }>,
  haEntityStates: Record<string, string>,
  haEnabled: boolean,
  mqttBooleans: Record<string, unknown>
): Record<string, string> {
  const states: Record<string, string> = {}
  for (const toggle of headerToggles) {
    if (haEnabled && haEntityStates[toggle.entity] !== undefined) {
      states[toggle.id] = haEntityStates[toggle.entity] === 'on' ? 'on' : 'off'
    } else {
      let val = mqttBooleans[toggle.id]
      if (typeof val === 'string') val = val === 'true' || val === '1'
      else if (typeof val === 'number') val = val !== 0
      states[toggle.id] = val ? 'on' : 'off'
    }
  }
  return states
}

const mockStates: Record<string, string> = {
  'sensor.temp': '23.5',
  'sensor.humidity': '65',
  'binary_sensor.motion': 'on',
  'sensor.unavailable': 'unavailable',
  'switch.light': 'on',
  'cover.blind': 'open',
  'weather.home': 'sunny',
}

const mockAttrs: Record<string, Record<string, unknown>> = {
  'sensor.temp': { friendly_name: 'Temperature', unit_of_measurement: '°C' },
  'sensor.humidity': { friendly_name: 'Humidity', unit_of_measurement: '%' },
  'binary_sensor.motion': { friendly_name: 'Motion' },
  'cover.blind': { friendly_name: 'Living Room Blind', current_position: 75 },
  'weather.home': {
    friendly_name: 'Home Weather',
    temperature: 22,
    temperature_unit: '°C',
    forecast: [{ datetime: '2024-01-02', temperature: 24, condition: 'cloudy' }],
  },
}

describe('filterSensors', () => {
  it('returns sensors and binary_sensors', () => {
    const result = filterSensors(mockStates, mockAttrs)
    expect(result).toHaveLength(3)
  })

  it('excludes unavailable sensors', () => {
    const result = filterSensors(mockStates, mockAttrs)
    const ids = result.map((s) => s.entity_id)
    expect(ids).not.toContain('sensor.unavailable')
  })

  it('excludes non-sensor domains', () => {
    const result = filterSensors(mockStates, mockAttrs)
    const ids = result.map((s) => s.entity_id)
    expect(ids).not.toContain('switch.light')
  })

  it('uses friendly_name from attributes', () => {
    const result = filterSensors(mockStates, mockAttrs)
    const temp = result.find((s) => s.entity_id === 'sensor.temp')
    expect(temp?.name).toBe('Temperature')
    expect(temp?.unit).toBe('°C')
  })
})

describe('filterCovers', () => {
  it('returns only cover entities', () => {
    const result = filterCovers(mockStates, mockAttrs)
    expect(result).toHaveLength(1)
    expect(result[0].entity_id).toBe('cover.blind')
  })

  it('extracts position from attributes', () => {
    const result = filterCovers(mockStates, mockAttrs)
    expect(result[0].position).toBe(75)
  })

  it('defaults position to 0 if missing', () => {
    const attrs = { 'cover.test': { friendly_name: 'Test' } }
    const states = { 'cover.test': 'open' }
    const result = filterCovers(states, attrs)
    expect(result[0].position).toBe(0)
  })
})

describe('filterWeather', () => {
  it('returns weather entity', () => {
    const result = filterWeather(mockStates, mockAttrs)
    expect(result).not.toBeNull()
    expect(result?.entity_id).toBe('weather.home')
  })

  it('extracts temperature and unit', () => {
    const result = filterWeather(mockStates, mockAttrs)
    expect(result?.temperature).toBe(22)
    expect(result?.unit).toBe('°C')
  })

  it('returns null if no weather entity', () => {
    const result = filterWeather({}, {})
    expect(result).toBeNull()
  })
})

describe('computeToggleStates', () => {
  const toggles = [
    { id: 'only_charging', entity: 'input_boolean.only_charging' },
    { id: 'no_feed', entity: 'input_boolean.no_feed' },
  ]

  it('uses HA entity state when HA enabled', () => {
    const states = computeToggleStates(
      toggles,
      { 'input_boolean.only_charging': 'on', 'input_boolean.no_feed': 'off' },
      true,
      {}
    )
    expect(states.only_charging).toBe('on')
    expect(states.no_feed).toBe('off')
  })

  it('falls back to MQTT booleans when HA disabled', () => {
    const states = computeToggleStates(toggles, {}, false, {
      only_charging: true,
      no_feed: false,
    })
    expect(states.only_charging).toBe('on')
    expect(states.no_feed).toBe('off')
  })

  it('handles string values from MQTT', () => {
    const states = computeToggleStates(toggles, {}, false, {
      only_charging: 'true',
      no_feed: 'false',
    })
    expect(states.only_charging).toBe('on')
    expect(states.no_feed).toBe('off')
  })

  it('handles numeric values from MQTT', () => {
    const states = computeToggleStates(toggles, {}, false, {
      only_charging: 1,
      no_feed: 0,
    })
    expect(states.only_charging).toBe('on')
    expect(states.no_feed).toBe('off')
  })

  it('defaults to off when no data', () => {
    const states = computeToggleStates(toggles, {}, false, {})
    expect(states.only_charging).toBe('off')
    expect(states.no_feed).toBe('off')
  })
})
