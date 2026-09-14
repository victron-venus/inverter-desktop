import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createI18n } from 'vue-i18n'
import en from '../messages.en'
import ru from '../messages.ru'
import RetainedPluginData from './RetainedPluginData.vue'
import { createRetainedPluginData } from './useRetainedPluginData'
import type { RetainedPluginDataRecord, RetainedPluginDataSnapshot } from './types'

const native = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: native.invoke }))

const orphan: RetainedPluginDataRecord = {
  record_id: 'a'.repeat(64),
  revision: '1'.repeat(64),
  bytes: 4096,
  plugin_id: null,
}
const installed: RetainedPluginDataRecord = {
  record_id: 'b'.repeat(64),
  revision: '2'.repeat(64),
  bytes: 128,
  plugin_id: 'example.disabled',
}
let inventory: RetainedPluginDataSnapshot
let allowed: boolean
let wrapper: VueWrapper | undefined
const controllers: Array<ReturnType<typeof createRetainedPluginData>> = []

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (value: unknown) => void
  const promise = new Promise<T>((done, fail) => {
    resolve = done
    reject = fail
  })
  return { promise, resolve, reject }
}
function controller() {
  const value = createRetainedPluginData(
    () => 'Stored data unavailable',
    () => allowed
  )
  controllers.push(value)
  return value
}
function calls(command: string) {
  return native.invoke.mock.calls.filter(([name]) => name === command)
}
function button(label: string) {
  const found = wrapper?.findAll('button').find((item) => item.text() === label)
  if (!found) throw new Error(`Missing button: ${label}`)
  return found
}
async function open(locale = 'en') {
  const value = controller()
  wrapper = mount(RetainedPluginData, {
    props: { controller: value, disabled: false },
    global: { plugins: [createI18n({ legacy: false, locale, messages: { en, ru } })] },
  })
  await value.open()
  await flushPromises()
  return value
}

beforeEach(() => {
  allowed = true
  inventory = {
    records: [structuredClone(orphan)],
    total_bytes: 4160,
    max_records: 128,
    max_bytes: 8_388_608,
  }
  native.invoke.mockReset().mockImplementation(async (command: string) => {
    if (command === 'get_retained_plugin_data') return structuredClone(inventory)
    if (command === 'delete_retained_plugin_data') return undefined
    throw new Error(`Unexpected IPC: ${command}`)
  })
})
afterEach(() => {
  wrapper?.unmount()
  wrapper = undefined
  for (const value of controllers) value.close()
  controllers.length = 0
})

describe('desktop retained plugin data', () => {
  it('loads only after an explicit open and shows native total usage including pending writes', async () => {
    const value = controller()
    expect(native.invoke).not.toHaveBeenCalled()
    await value.refresh()
    expect(native.invoke).not.toHaveBeenCalled()
    await open()
    expect(calls('get_retained_plugin_data')).toHaveLength(1)
    expect(wrapper?.text()).toContain('4,160 of 8,388,608 bytes used; up to 128 records.')
    expect(wrapper?.text()).toContain('Storage usage also includes pending writes.')
    expect(wrapper?.text()).toContain('Unidentified stored data')
    expect(wrapper?.text()).toContain('aaaaaaaaaaaa…')
    expect(wrapper?.text()).toContain('4,096 bytes')
    expect(wrapper?.text()).not.toContain('example.monitor')
  })

  it('protects every installed owner, escapes metadata, and never opens record contents', async () => {
    inventory.records = [
      structuredClone(installed),
      { ...orphan, plugin_id: '<img src=x onerror=alert(1)>' },
    ]
    const value = await open()
    expect(wrapper?.text()).toContain('example.disabled')
    expect(wrapper?.text()).toContain('<img src=x onerror=alert(1)>')
    expect(wrapper?.find('img').exists()).toBe(false)
    expect(wrapper?.text()).toContain('Uninstall the plugin before deleting its data.')
    for (const item of wrapper
      ?.findAll('button')
      .filter((item) => item.text() === 'Delete stored data…') ?? []) {
      expect(item.attributes('disabled')).toBeDefined()
    }
    value.requestDeletion(installed.record_id)
    value.requestDeletion(orphan.record_id)
    await value.remove()
    expect(value.confirmation.value).toBeNull()
    expect(native.invoke.mock.calls.map(([name]) => name)).toEqual(['get_retained_plugin_data'])
  })

  it('requires concrete irreversible deletion consent and supports cancellation', async () => {
    const value = await open()
    await value.remove()
    expect(calls('delete_retained_plugin_data')).toHaveLength(0)
    await button('Delete stored data…').trigger('click')
    expect(wrapper?.text()).toContain(`Permanently delete stored data record ${orphan.record_id}`)
    expect(wrapper?.text()).toContain('Its settings and secrets cannot be recovered.')
    expect(calls('delete_retained_plugin_data')).toHaveLength(0)
    await button('Cancel').trigger('click')
    expect(value.confirmation.value).toBeNull()
    await value.remove()
    expect(calls('delete_retained_plugin_data')).toHaveLength(0)
  })

  it('deletes the reviewed ciphertext revision once and refreshes usage after success', async () => {
    const value = await open()
    const pending = deferred<void>()
    native.invoke.mockImplementation(async (command: string) => {
      if (command === 'delete_retained_plugin_data') return pending.promise
      return structuredClone(inventory)
    })
    value.requestDeletion(orphan.record_id)
    const deleting = value.remove()
    await value.remove()
    expect(calls('delete_retained_plugin_data')).toHaveLength(1)
    expect(native.invoke).toHaveBeenCalledWith('delete_retained_plugin_data', {
      recordId: orphan.record_id,
      revision: orphan.revision,
    })
    expect(value.confirmation.value).toBeNull()
    expect(value.busy.value).toBe(true)
    inventory.records = []
    inventory.total_bytes = 64
    pending.resolve()
    await deleting
    await flushPromises()
    expect(calls('get_retained_plugin_data')).toHaveLength(2)
    expect(wrapper?.text()).toContain('No stored data records.')
    expect(wrapper?.text()).toContain('64 of 8,388,608 bytes used')
    expect(value.busy.value).toBe(false)
  })

  it('drops stale consent and fails closed after a deletion error until a fresh read and confirmation', async () => {
    const value = await open()
    native.invoke.mockRejectedValueOnce(
      new Error(`<script>revision changed</script>${'x'.repeat(600)}`)
    )
    value.requestDeletion(orphan.record_id)
    await value.remove()
    await flushPromises()
    expect(value.snapshot.value).toBeNull()
    expect(value.confirmation.value).toBeNull()
    expect(wrapper?.find('[role="alert"]').text()).toHaveLength(512)
    expect(wrapper?.find('script').exists()).toBe(false)
    await value.remove()
    expect(calls('delete_retained_plugin_data')).toHaveLength(1)
    inventory.records[0].revision = '3'.repeat(64)
    await button('Refresh').trigger('click')
    await flushPromises()
    expect(wrapper?.find('[role="alert"]').exists()).toBe(false)
    await value.remove()
    expect(calls('delete_retained_plugin_data')).toHaveLength(1)
    value.requestDeletion(orphan.record_id)
    expect(value.confirmation.value?.revision).toBe('3'.repeat(64))
  })

  it('coalesces refresh requests into one in-flight scan and one follow-up', async () => {
    const value = await open()
    const pending = deferred<RetainedPluginDataSnapshot>()
    native.invoke.mockReturnValueOnce(pending.promise)
    const refreshing = value.refresh()
    for (let index = 0; index < 100; index += 1) void value.refresh()
    expect(calls('get_retained_plugin_data')).toHaveLength(2)
    pending.resolve(inventory)
    await refreshing
    expect(calls('get_retained_plugin_data')).toHaveLength(3)
  })

  it('ignores a stale scan after auth teardown and serializes the next open behind it', async () => {
    const value = controller()
    const pending = deferred<RetainedPluginDataSnapshot>()
    native.invoke.mockReturnValueOnce(pending.promise)
    const first = value.open()
    allowed = false
    value.close()
    allowed = true
    inventory.records = []
    const second = value.open()
    expect(calls('get_retained_plugin_data')).toHaveLength(1)
    pending.resolve({ ...inventory, records: [orphan] })
    await Promise.all([first, second])
    expect(calls('get_retained_plugin_data')).toHaveLength(2)
    expect(value.snapshot.value?.records).toEqual([])
  })

  it('drops deletion completion and errors after auth revocation without starting another scan', async () => {
    const value = await open()
    const pending = deferred<void>()
    native.invoke.mockReturnValueOnce(pending.promise)
    value.requestDeletion(orphan.record_id)
    const deleting = value.remove()
    allowed = false
    value.close()
    pending.reject(new Error('old session failure'))
    await deleting
    expect(value.opened.value).toBe(false)
    expect(value.snapshot.value).toBeNull()
    expect(value.confirmation.value).toBeNull()
    expect(value.error.value).toBeNull()
    expect(calls('get_retained_plugin_data')).toHaveLength(1)
  })

  it('requires new deletion consent after authentication changes even for the same record revision', async () => {
    const value = await open()
    value.requestDeletion(orphan.record_id)
    expect(value.confirmation.value?.record_id).toBe(orphan.record_id)
    allowed = false
    value.close()
    allowed = true
    await value.open()
    await value.remove()
    expect(value.confirmation.value).toBeNull()
    expect(calls('delete_retained_plugin_data')).toHaveLength(0)
  })

  it('clears a confirmation when ownership or revision is refreshed', async () => {
    const value = await open()
    value.requestDeletion(orphan.record_id)
    inventory.records[0].plugin_id = 'example.newly-installed'
    inventory.records[0].revision = '4'.repeat(64)
    value.invalidate()
    await value.refresh()
    await value.remove()
    value.requestDeletion(orphan.record_id)
    expect(value.confirmation.value).toBeNull()
    expect(calls('delete_retained_plugin_data')).toHaveLength(0)
  })

  it('honors the parent busy boundary for UI and direct controller requests', async () => {
    const value = await open()
    allowed = false
    await wrapper?.setProps({ disabled: true })
    await button('Delete stored data…').trigger('click')
    await value.refresh()
    value.requestDeletion(orphan.record_id)
    await value.remove()
    expect(value.confirmation.value).toBeNull()
    expect(calls('get_retained_plugin_data')).toHaveLength(1)
    expect(calls('delete_retained_plugin_data')).toHaveLength(0)
  })

  it('clears data and ignores pending refreshes after the panel unmounts', async () => {
    const value = await open()
    const pending = deferred<RetainedPluginDataSnapshot>()
    native.invoke.mockReturnValueOnce(pending.promise)
    const refreshing = value.refresh()
    wrapper?.unmount()
    wrapper = undefined
    pending.resolve(inventory)
    await refreshing
    expect(value.opened.value).toBe(false)
    expect(value.snapshot.value).toBeNull()
  })

  it('shows bounded load errors and recovers through explicit refresh', async () => {
    native.invoke.mockRejectedValueOnce(new Error('Inventory unavailable'))
    await open()
    expect(wrapper?.text()).toContain('Inventory unavailable')
    await button('Refresh').trigger('click')
    await flushPromises()
    expect(wrapper?.find('[role="alert"]').exists()).toBe(false)
    expect(wrapper?.text()).toContain('Unidentified stored data')
  })

  it('localizes unidentified records and destructive consent in Russian', async () => {
    await open('ru')
    expect(wrapper?.text()).toContain('Неопознанные сохранённые данные')
    await button('Удалить сохранённые данные…').trigger('click')
    expect(wrapper?.text()).toContain('Её настройки и секреты нельзя будет восстановить.')
    expect(button('Удалить данные безвозвратно').exists()).toBe(true)
  })
})
