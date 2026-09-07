import { describe, expect, it } from 'vitest'
import { needsSetup, type AppConfig } from '../config'

function cfg(partial: Partial<AppConfig> = {}): AppConfig {
  return {
    mqtt_host: 'Cerbo',
    mqtt_port: 1883,
    setup_completed: false,
    ...partial,
  }
}

describe('needsSetup', () => {
  it('is true when config is missing', () => {
    expect(needsSetup(null)).toBe(true)
    expect(needsSetup(undefined)).toBe(true)
  })

  it('is true when setup_completed is false or unset', () => {
    expect(needsSetup(cfg({ setup_completed: false }))).toBe(true)
    expect(needsSetup(cfg({ setup_completed: undefined }))).toBe(true)
  })

  it('is false when setup_completed is true', () => {
    expect(needsSetup(cfg({ setup_completed: true }))).toBe(false)
  })

  it('does not infer completion from default mqtt_host alone', () => {
    expect(needsSetup(cfg({ mqtt_host: 'Cerbo', setup_completed: false }))).toBe(true)
  })
})
