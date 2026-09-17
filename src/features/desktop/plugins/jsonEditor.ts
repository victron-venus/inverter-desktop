import { toRaw } from 'vue'
import type { JsonEditorSchema, PluginSettingsChoices } from './types'
export function initialJsonValue(schema: JsonEditorSchema): unknown {
  // IPC schemas become reactive in the settings store; structuredClone rejects Vue proxies.
  if ('const' in schema) return structuredClone(toRaw(schema.const))
  if ('default' in schema) return structuredClone(toRaw(schema.default))
  if (schema.enum?.length) return schema.enum[0]
  if (schema.type === 'object')
    return Object.fromEntries(
      Object.entries(schema.properties ?? {})
        .filter(
          ([key, field]) => schema.required?.includes(key) || 'default' in field || 'const' in field
        )
        .map(([key, field]) => [key, initialJsonValue(field)])
    )
  if (schema.type === 'array') return []
  if (schema.type === 'boolean') return false
  if (schema.type === 'number' || schema.type === 'integer') return schema.minimum ?? 0
  return ''
}
export function fieldChoices(
  source: string | undefined,
  prefixes: string[] | undefined,
  choices: PluginSettingsChoices
) {
  return (source ? (choices.sources[source] ?? []) : []).filter(
    (option) => !prefixes?.length || prefixes.some((prefix) => option.value.startsWith(prefix))
  )
}

/** Recognize editable shape without dropping values unknown to a newer package schema. */
export function hasEditableJsonShape(value: unknown, schema: JsonEditorSchema): boolean {
  if (schema.type === 'object') {
    if (!value || typeof value !== 'object' || Array.isArray(value)) return false
    return Object.entries(value).every(([key, child]) => {
      const field = schema.properties?.[key]
      const extra = schema.additionalProperties
      return field
        ? hasEditableJsonShape(child, field)
        : typeof extra !== 'object' || hasEditableJsonShape(child, extra)
    })
  }
  if (schema.type === 'array')
    return (
      Array.isArray(value) &&
      !!schema.items &&
      value.every((child) => hasEditableJsonShape(child, schema.items as JsonEditorSchema))
    )
  if (schema.type === 'string') return typeof value === 'string'
  if (schema.type === 'boolean') return typeof value === 'boolean'
  return (
    typeof value === 'number' &&
    Number.isFinite(value) &&
    (schema.type !== 'integer' || Number.isSafeInteger(value))
  )
}
