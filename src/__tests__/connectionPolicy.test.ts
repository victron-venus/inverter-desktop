import { describe, expect, it } from 'vitest'
import {
  chooseStartupSource,
  gatewayConfigError,
  isIgwConfigured,
  isMqttConfigured,
  MQTT_CONNECT_WATCHDOG_MS,
  MQTT_OFFLINE_DELAY_MS,
  MQTT_RECOVERY_PROBE_MS,
  mqttReconnectDelayMs,
  shouldWatchdogFailoverToIgw,
} from '../connectionPolicy'

describe('isMqttConfigured', () => {
  it('requires a non-empty host', () => {
    expect(isMqttConfigured({ mqtt_host: 'Cerbo' })).toBe(true)
    expect(isMqttConfigured({ mqtt_host: '  ' })).toBe(false)
    expect(isMqttConfigured({ mqtt_host: '' })).toBe(false)
    expect(isMqttConfigured({})).toBe(false)
  })
})

describe('isIgwConfigured', () => {
  const native = {
    gateway_enabled: true,
    gateway_url: 'https://igw.example',
    gateway_api_token: 'read-token',
  }

  it.each([undefined, null, '', '  '])(
    'accepts native HTTPS with absent Access fields (%s)',
    (empty) => {
      expect(
        isIgwConfigured({
          ...native,
          gateway_access_client_id: empty,
          gateway_access_client_secret: empty,
        })
      ).toBe(true)
    }
  )

  it('accepts paired Access credentials and leaves bearer requirements to the server', () => {
    expect(
      isIgwConfigured({
        ...native,
        gateway_api_token: null,
        gateway_access_client_id: 'id',
        gateway_access_client_secret: 'secret',
      })
    ).toBe(true)
    expect(isIgwConfigured({ ...native, gateway_api_token: null })).toBe(true)
  })

  it.each([
    { gateway_access_client_id: 'id', gateway_access_client_secret: undefined },
    { gateway_access_client_id: null, gateway_access_client_secret: 'secret' },
    { gateway_access_client_id: 'id', gateway_access_client_secret: '  ' },
    { gateway_access_client_id: '  ', gateway_access_client_secret: 'secret' },
  ])('rejects incomplete Access credentials', (pair) => {
    const config = { ...native, ...pair }
    expect(isIgwConfigured(config)).toBe(false)
    expect(gatewayConfigError(config)).toBe(
      'Provide both Cloudflare Access fields or leave both blank'
    )
  })

  it('requires an enabled gateway and non-empty URL', () => {
    expect(isIgwConfigured({ ...native, gateway_enabled: false })).toBe(false)
    expect(isIgwConfigured({ gateway_enabled: true })).toBe(false)
    expect(isIgwConfigured({ ...native, gateway_url: '  ' })).toBe(false)
    expect(gatewayConfigError({ gateway_url: '  ' })).toBe('Gateway URL is required')
  })
})

describe('chooseStartupSource', () => {
  it('prefers reachable MQTT when both configured', () => {
    expect(
      chooseStartupSource({ mqttConfigured: true, igwConfigured: true, mqttReachable: true })
    ).toBe('mqtt')
  })

  it('falls back to IGW when MQTT unreachable and both configured', () => {
    expect(
      chooseStartupSource({ mqttConfigured: true, igwConfigured: true, mqttReachable: false })
    ).toBe('igw')
  })

  it('uses single transport when only one is configured', () => {
    expect(
      chooseStartupSource({ mqttConfigured: true, igwConfigured: false, mqttReachable: false })
    ).toBe('mqtt')
    expect(
      chooseStartupSource({ mqttConfigured: false, igwConfigured: true, mqttReachable: false })
    ).toBe('igw')
  })

  it('returns none when nothing is configured', () => {
    expect(
      chooseStartupSource({ mqttConfigured: false, igwConfigured: false, mqttReachable: false })
    ).toBe('none')
  })
})

describe('mqttReconnectDelayMs', () => {
  it('starts near 5s and caps at 60s', () => {
    expect(mqttReconnectDelayMs(0)).toBe(5_000)
    expect(mqttReconnectDelayMs(1)).toBe(10_000)
    expect(mqttReconnectDelayMs(2)).toBe(20_000)
    expect(mqttReconnectDelayMs(3)).toBe(40_000)
    expect(mqttReconnectDelayMs(4)).toBe(60_000)
    expect(mqttReconnectDelayMs(10)).toBe(60_000)
  })
})

describe('MQTT_RECOVERY_PROBE_MS', () => {
  it('is once per minute', () => {
    expect(MQTT_RECOVERY_PROBE_MS).toBe(60_000)
  })
})

describe('MQTT_CONNECT_WATCHDOG_MS', () => {
  it('is about 15 seconds', () => {
    expect(MQTT_CONNECT_WATCHDOG_MS).toBe(15_000)
  })
})

describe('MQTT_OFFLINE_DELAY_MS', () => {
  it('is 10 seconds', () => {
    expect(MQTT_OFFLINE_DELAY_MS).toBe(10_000)
  })
})

describe('shouldWatchdogFailoverToIgw', () => {
  it('fires only in dual-path MQTT without a real connection', () => {
    expect(
      shouldWatchdogFailoverToIgw({
        dualPath: true,
        dataSource: 'mqtt',
        mqttConnected: false,
      })
    ).toBe(true)
    expect(
      shouldWatchdogFailoverToIgw({
        dualPath: true,
        dataSource: 'mqtt',
        mqttConnected: true,
      })
    ).toBe(false)
    expect(
      shouldWatchdogFailoverToIgw({
        dualPath: true,
        dataSource: 'igw',
        mqttConnected: false,
      })
    ).toBe(false)
    expect(
      shouldWatchdogFailoverToIgw({
        dualPath: false,
        dataSource: 'mqtt',
        mqttConnected: false,
      })
    ).toBe(false)
  })
})
