import { invoke } from '@tauri-apps/api/core'
import { ref } from 'vue'
import type { RetainedPluginDataRecord, RetainedPluginDataSnapshot } from './types'

/** Explicit inventory reads only; worker contribution events never schedule a disk scan. */
export function createRetainedPluginData(fallbackError: () => string, canUse: () => boolean) {
  const opened = ref(false)
  const snapshot = ref<RetainedPluginDataSnapshot | null>(null)
  const confirmation = ref<RetainedPluginDataRecord | null>(null)
  const busy = ref(false)
  const error = ref<string | null>(null)
  let generation = 0
  let requested = false
  let operation: Promise<void> | undefined

  function current(request: number) {
    return opened.value && request === generation && canUse()
  }

  function message(value: unknown) {
    if (value instanceof Error) return value.message.slice(0, 512) || fallbackError()
    if (typeof value === 'string') return value.slice(0, 512) || fallbackError()
    return fallbackError()
  }

  function cancelDeletion() {
    confirmation.value = null
  }

  function invalidate() {
    generation += 1
    requested = false
    snapshot.value = null
    error.value = null
    cancelDeletion()
  }

  function close() {
    opened.value = false
    invalidate()
    busy.value = false
  }

  async function read(request: number) {
    try {
      const value = await invoke<RetainedPluginDataSnapshot>('get_retained_plugin_data')
      if (current(request)) snapshot.value = value
    } catch (error_) {
      if (current(request)) {
        snapshot.value = null
        error.value = message(error_)
      }
    }
  }

  async function run(deleting?: RetainedPluginDataRecord) {
    try {
      if (deleting) {
        const request = generation
        try {
          await invoke('delete_retained_plugin_data', {
            recordId: deleting.record_id,
            revision: deleting.revision,
          })
          if (current(request)) requested = true
        } catch (error_) {
          if (current(request)) {
            snapshot.value = null
            error.value = message(error_)
          }
        }
      }
      while (requested && opened.value && canUse()) {
        requested = false
        await read(generation)
      }
    } finally {
      operation = undefined
      busy.value = false
    }
  }

  function refresh(): Promise<void> {
    if (!opened.value || !canUse()) return Promise.resolve()
    cancelDeletion()
    error.value = null
    requested = true
    busy.value = true
    operation ??= run()
    return operation
  }

  async function open() {
    if (!canUse()) return
    opened.value = true
    await refresh()
  }

  function requestDeletion(recordId: string) {
    if (!opened.value || busy.value || !canUse()) return
    const record = snapshot.value?.records.find((entry) => entry.record_id === recordId)
    if (record?.plugin_id === null) confirmation.value = { ...record }
  }

  function remove(): Promise<void> {
    if (!opened.value || busy.value || operation || !canUse()) return Promise.resolve()
    const selected = confirmation.value
    const record = snapshot.value?.records.find((entry) => entry.record_id === selected?.record_id)
    if (!selected || !record || record.plugin_id !== null || record.revision !== selected.revision)
      return Promise.resolve()
    cancelDeletion()
    error.value = null
    busy.value = true
    operation = run({ ...record })
    return operation
  }

  return {
    opened,
    snapshot,
    confirmation,
    busy,
    error,
    open,
    close,
    refresh,
    invalidate,
    cancelDeletion,
    requestDeletion,
    remove,
  }
}

export type RetainedPluginDataController = ReturnType<typeof createRetainedPluginData>
