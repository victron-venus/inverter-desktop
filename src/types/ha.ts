export interface HaSensorDisplay {
  entity_id: string
  name: string
  state: string
  unit: string
}

export interface HaNumberDisplay {
  entity_id: string
  name: string
  value: number
  min: number
  max: number
  step: number
  unit: string
}

export interface HaCoverDisplay {
  entity_id: string
  name: string
  position: number
  /** HA cover state: open / closed / opening / closing / unavailable / unknown */
  state?: string
}

export interface HaMediaPlayerDisplay {
  entity_id: string
  name: string
  state: string
}

export interface HaWeatherDisplay {
  entity_id: string
  name: string
  state: string
  temperature: number | null
  unit: string
  forecast: Array<Record<string, unknown>>
}

export interface HaSceneDisplay {
  entity_id: string
  name: string
}

export interface HaFilteredData {
  sensors: HaSensorDisplay[]
  numbers: HaNumberDisplay[]
  covers: HaCoverDisplay[]
  media_players: HaMediaPlayerDisplay[]
  scenes: HaSceneDisplay[]
  weather: HaWeatherDisplay | null
  /** When true, replace sensors from a coalesced live update or initial snapshot. */
  refresh_sensors?: boolean
}
