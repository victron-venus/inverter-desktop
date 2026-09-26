import { describe, expect, it } from 'vitest'
import { connectionStatuses } from './connectionStatuses'
import type { PluginSnapshot } from './types'

function worker(name: string, id: string, title: string, connected = true): PluginSnapshot {
  return {
    plugin_id: `inverter-desktop.${name}`,
    instance_id: `worker-${name}`,
    state: 'running',
    generation: 1,
    restart_count: 0,
    last_error: null,
    contributions: [],
    presentation: [{ kind: 'connection', id, title, connected }],
  }
}
const frigate = () => worker('frigate', 'frigate-mqtt', 'Frigate MQTT')
const kerberos = () => worker('kerberos', 'kerberos-mqtt', 'Kerberos MQTT')
const ha = () => worker('home-assistant', 'ha-connection', 'Home Assistant')
const available = (plugin: PluginSnapshot) => plugin.state === 'running' && !!plugin.instance_id
const configured = [
  { plugin_id: 'inverter-desktop.frigate', enabled: true },
  { plugin_id: 'inverter-desktop.kerberos', enabled: true },
]

describe('connection indicators', () => {
  it('summarizes camera MQTT once and abbreviates the separate HA connection', () => {
    const result = connectionStatuses([frigate(), ha(), kerberos()], available, configured)
    expect(result.map(({ label, connected }) => ({ label, connected }))).toEqual([
      { label: 'HA MQTT', connected: true },
      { label: 'HA', connected: true },
    ])
    expect(result[0].details).toBe('Frigate MQTT: Connected; Kerberos MQTT: Connected')
    expect(result[1].details).toBe('Home Assistant: Connected')
  })

  it('keeps partial disconnect, cleared crash projections and an absent expected worker offline', () => {
    const failed = kerberos()
    failed.presentation = [
      { kind: 'connection', id: 'kerberos-mqtt', title: 'Kerberos MQTT', connected: false },
    ]
    expect(connectionStatuses([frigate(), failed], available, configured)[0]).toMatchObject({
      connected: false,
      details: 'Frigate MQTT: Connected; Kerberos MQTT: Disconnected',
    })
    for (const state of ['restarting', 'failed', 'stopped'] as const) {
      failed.state = state
      failed.presentation = []
      expect(connectionStatuses([frigate(), failed], available, configured)[0]).toMatchObject({
        connected: false,
        details: 'Frigate MQTT: Connected; Kerberos MQTT: Unavailable',
      })
    }
    expect(connectionStatuses([frigate()], available, configured)[0].connected).toBe(false)
  })

  it('excludes explicitly disabled participants even while a stopped snapshot remains', () => {
    const disabled = kerberos()
    disabled.state = 'stopped'
    disabled.presentation = []
    const declarations = [configured[0], { ...configured[1], enabled: false }]
    expect(connectionStatuses([frigate(), disabled], available, declarations)).toEqual([
      expect.objectContaining({
        label: 'HA MQTT',
        connected: true,
        details: 'Frigate MQTT: Connected',
      }),
    ])
    expect(
      connectionStatuses(
        [frigate(), disabled],
        available,
        declarations.map((item) => ({ ...item, enabled: false }))
      )
    ).toEqual([])
    expect(connectionStatuses([], available, [])).toEqual([])
  })

  it('keeps expected HA visible while unavailable and never treats a stale snapshot as connected', () => {
    expect(
      connectionStatuses([], available, [{ plugin_id: 'inverter-desktop.home-assistant' }])
    ).toEqual([
      expect.objectContaining({
        label: 'HA',
        connected: false,
        details: 'Home Assistant: Unavailable',
      }),
    ])
    const result = connectionStatuses([frigate(), kerberos(), ha()], () => false, configured)
    expect(result.every((entry) => !entry.connected)).toBe(true)
  })

  it('preserves unrelated connections instead of grouping by a misleading title', () => {
    const other = worker('third-party', 'frigate-mqtt', 'Frigate MQTT')
    const result = connectionStatuses([frigate(), other], available, [])
    expect(result.map((entry) => entry.label)).toEqual(['HA MQTT', 'Frigate MQTT'])
  })
})
