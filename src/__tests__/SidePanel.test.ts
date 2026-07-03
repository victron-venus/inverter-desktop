import { mount } from '@vue/test-utils'
import { describe, expect, it, vi } from 'vitest'
import SidePanel from '../components/SidePanel.vue'

vi.mock('vue-i18n', () => ({
  useI18n: () => ({ t: (key: string) => key }),
}))

const baseProps = {
  evCharging: '0',
  evPower: '0',
  evPowerWatts: 0,
  evChargingKw: 0,
  evLoadPower: 0,
  carSoc: 80,
  waterLevel: 42,
  waterValve: false,
  pumpSwitch: false,
  pumpSwitchEntity: 'switch.pump',
  waterValveEntity: 'switch.valve',
  homeButtons: [],
  buttonStates: {},
  haSensors: [{ entity_id: 'sensor.temp', name: 'Temp', state: '23', unit: '°C' }],
  haNumbers: [
    { entity_id: 'number.limit', name: 'Limit', value: 50, min: 0, max: 100, step: 1, unit: '%' },
  ],
  haCovers: [],
  haMediaPlayers: [],
  haScenes: [],
  haWeather: null,
  appConfig: {
    show_ha_sensors: true,
    show_ha_numbers: true,
    show_ha_covers: true,
    show_ha_media: true,
    show_ha_scenes: true,
    show_ha_weather: true,
    show_ev: true,
    show_washer: true,
    show_dryer: true,
    show_dishwasher: true,
    show_home_section: true,
    show_console: true,
  },
}

describe('SidePanel', () => {
  it('renders with minimal props', () => {
    const wrapper = mount(SidePanel, { props: baseProps })
    expect(wrapper.exists()).toBe(true)
  })

  it('shows EV section when showEv true', () => {
    const wrapper = mount(SidePanel, {
      props: { ...baseProps, showEv: true },
    })
    expect(wrapper.text()).toContain('sections.ev')
    expect(wrapper.text()).toContain('80%')
  })

  it('hides EV section when showEv false', () => {
    const wrapper = mount(SidePanel, {
      props: { ...baseProps, showEv: false },
    })
    expect(wrapper.text()).not.toContain('sections.ev')
  })

  it('shows water section', () => {
    const wrapper = mount(SidePanel, { props: baseProps })
    expect(wrapper.text()).toContain('sections.water')
    expect(wrapper.text()).toContain('42 cm')
  })

  it('shows pump and valve buttons', () => {
    const wrapper = mount(SidePanel, { props: baseProps })
    expect(wrapper.text()).toContain('sections.pump')
    expect(wrapper.text()).toContain('sections.valve')
  })

  it('shows home buttons when provided', () => {
    const wrapper = mount(SidePanel, {
      props: {
        ...baseProps,
        showHomeSection: true,
        homeButtons: [
          { id: 'btn1', label: 'Button 1', entity: 'switch.one' },
          { id: 'btn2', label: 'Button 2', entity: 'switch.two' },
        ],
      },
    })
    expect(wrapper.text()).toContain('Button 1')
    expect(wrapper.text()).toContain('Button 2')
  })

  it('hides home section when showHomeSection false', () => {
    const wrapper = mount(SidePanel, {
      props: {
        ...baseProps,
        showHomeSection: false,
        homeButtons: [{ id: 'btn1', label: 'Btn', entity: 'switch.one' }],
      },
    })
    expect(wrapper.text()).not.toContain('Btn')
  })

  it('shows sensors header but collapsed by default', () => {
    const wrapper = mount(SidePanel, { props: baseProps })
    expect(wrapper.text()).toContain('sections.sensors')
    expect(wrapper.text()).not.toContain('23°C')
  })

  it('expands sensors on header click', async () => {
    const wrapper = mount(SidePanel, { props: baseProps })
    const headers = wrapper.findAll('.classic-header')
    const sensorsHeader = headers.find((h) => h.text().includes('sections.sensors'))
    await sensorsHeader?.trigger('click')
    expect(wrapper.text()).toContain('23°C')
  })

  it('shows covers collapsed, expands on click', async () => {
    const wrapper = mount(SidePanel, {
      props: {
        ...baseProps,
        haCovers: [{ entity_id: 'cover.blind1', name: 'Living Room', position: 75 }],
      },
    })
    expect(wrapper.text()).toContain('sections.covers')
    expect(wrapper.text()).not.toContain('75%')
    const headers = wrapper.findAll('.classic-header')
    const coversHeader = headers.find((h) => h.text().includes('sections.covers'))
    await coversHeader?.trigger('click')
    expect(wrapper.text()).toContain('75%')
  })

  it('shows media collapsed, expands on click', async () => {
    const wrapper = mount(SidePanel, {
      props: {
        ...baseProps,
        haMediaPlayers: [
          { entity_id: 'media_player.tv', name: 'Living Room TV', state: 'playing' },
        ],
      },
    })
    expect(wrapper.text()).toContain('sections.media')
    expect(wrapper.text()).not.toContain('playing')
    const headers = wrapper.findAll('.classic-header')
    const mediaHeader = headers.find((h) => h.text().includes('sections.media'))
    await mediaHeader?.trigger('click')
    expect(wrapper.text()).toContain('playing')
  })

  it('shows scenes collapsed, expands on click', async () => {
    const wrapper = mount(SidePanel, {
      props: {
        ...baseProps,
        haScenes: [{ entity_id: 'scene.movie', name: 'Movie Night' }],
      },
    })
    expect(wrapper.text()).toContain('sections.scenes')
    expect(wrapper.text()).not.toContain('Movie Night')
    const headers = wrapper.findAll('.classic-header')
    const scenesHeader = headers.find((h) => h.text().includes('sections.scenes'))
    await scenesHeader?.trigger('click')
    expect(wrapper.text()).toContain('Movie Night')
  })

  it('shows weather when provided', () => {
    const wrapper = mount(SidePanel, {
      props: {
        ...baseProps,
        haWeather: {
          entity_id: 'weather.home',
          name: 'Home Weather',
          state: 'sunny',
          temperature: 22,
          unit: '°C',
          forecast: [{ datetime: '2024-01-02', temperature: 24, condition: 'cloudy' }],
        },
      },
    })
    expect(wrapper.text()).toContain('Home Weather')
    expect(wrapper.text()).toContain('22°C')
    expect(wrapper.text()).toContain('sunny')
  })

  it('hides weather when show_ha_weather false', () => {
    const wrapper = mount(SidePanel, {
      props: {
        ...baseProps,
        haWeather: {
          entity_id: 'weather.home',
          name: 'Home Weather',
          state: 'sunny',
          temperature: 22,
          unit: '°C',
          forecast: [],
        },
        appConfig: { ...(baseProps.appConfig as Record<string, boolean>), show_ha_weather: false },
      },
    })
    expect(wrapper.text()).not.toContain('Home Weather')
  })
})
