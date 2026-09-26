import type { PluginPresentation, PluginSnapshot } from './types'
import type { DesktopPluginConfig } from '../../../config'

type Connection = Extract<PluginPresentation, { kind: 'connection' }>
interface ConnectionStatus {
  key: string
  label: string
  connected: boolean
  details: string
}

const knownConnections = new Map([
  ['inverter-desktop.frigate', { id: 'frigate-mqtt', title: 'Frigate MQTT', mqtt: true }],
  ['inverter-desktop.kerberos', { id: 'kerberos-mqtt', title: 'Kerberos MQTT', mqtt: true }],
  ['inverter-desktop.ring', { id: 'ring-mqtt', title: 'Ring MQTT', mqtt: true }],
  [
    'inverter-desktop.home-assistant',
    { id: 'ha-connection', title: 'Home Assistant', mqtt: false },
  ],
])

/** A shared indicator summarizes independent workers; one failed worker must not look healthy. */
export function connectionStatuses(
  plugins: PluginSnapshot[],
  available: (plugin: PluginSnapshot) => boolean,
  configured: Pick<DesktopPluginConfig, 'plugin_id' | 'enabled'>[] | null
): ConnectionStatus[] {
  const mqtt: ConnectionStatus[] = []
  const others: ConnectionStatus[] = []
  const declarations = new Map((configured ?? []).map((plugin) => [plugin.plugin_id, plugin]))
  const snapshots = new Map(plugins.map((plugin) => [plugin.plugin_id, plugin]))
  const identities = new Set([...snapshots.keys(), ...declarations.keys()])
  for (const identity of identities) {
    if (declarations.get(identity)?.enabled === false) continue
    const plugin = snapshots.get(identity)
    const connections = (plugin?.presentation ?? []).filter(
      (item): item is Connection => item.kind === 'connection'
    )
    const known = knownConnections.get(identity)
    if (known) {
      // The host clears projections during worker restarts/failures. Keep that
      // participant offline until its connection is explicitly advertised again.
      const item = connections.find((connection) => connection.id === known.id)
      const entry = status(identity, plugin, known.id, known.title, item)
      if (known.mqtt) mqtt.push(entry)
      else others.push({ ...entry, label: 'HA' })
    }
    for (const item of connections) {
      if (item.id === known?.id) continue
      others.push(status(identity, plugin, item.id, item.title, item))
    }
  }
  return [
    ...(mqtt.length
      ? [
          {
            key: 'ha-mqtt',
            label: 'HA MQTT',
            connected: mqtt.every((entry) => entry.connected),
            details: mqtt.map((entry) => entry.details).join('; '),
          },
        ]
      : []),
    ...others,
  ]

  function status(
    identity: string,
    plugin: PluginSnapshot | undefined,
    id: string,
    title: string,
    item?: Connection
  ): ConnectionStatus {
    const usable = !!item && !!plugin && available(plugin)
    const connected = usable && item.connected
    return {
      key: JSON.stringify([identity, plugin?.instance_id ?? null, id]),
      label: title,
      connected,
      details: `${title}: ${usable ? (connected ? 'Connected' : 'Disconnected') : 'Unavailable'}`,
    }
  }
}
