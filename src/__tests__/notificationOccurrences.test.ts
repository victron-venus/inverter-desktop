import { beforeEach, describe, expect, it, vi } from 'vitest'
import {
  addNotification,
  type BannerNotification,
  clearNotifications,
  clearVictronBanners,
  dismissBanner,
  endNotificationOccurrence,
  markAllNotificationsRead,
  notifications,
  unreadNotificationCount,
  upsertNotification,
} from '../composables/useInverterState'
import { notificationTimestampMs } from '../utils'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))

const timestamp = '2026-11-17T18:47:00Z'
const alarm = (changes: Partial<BannerNotification> = {}): BannerNotification => ({
  id: 'victron-platform-0-1',
  source: 'victron',
  title: 'Battery alarm',
  body: 'Battery',
  level: 'alarm',
  ts: timestamp,
  ...changes,
})
function receive(notification = alarm()) {
  upsertNotification(notification, notificationTimestampMs(notification.ts))
}

beforeEach(clearNotifications)

describe('source notification occurrences', () => {
  it('updates repeated cell details without making a read occurrence unread or evicting other history', () => {
    addNotification('Other event', 'Keep this event')
    receive()
    const entryId = notifications.value[0].id
    markAllNotificationsRead()
    for (let update = 0; update < 150; update += 1) {
      receive(alarm({ title: 'Cell voltage alarm', body: `Cell detail ${update}` }))
    }
    expect(notifications.value).toHaveLength(2)
    expect(notifications.value[0]).toMatchObject({
      id: entryId,
      title: 'Cell voltage alarm',
      body: 'Cell detail 149',
      timestamp: Date.parse(timestamp),
      read: true,
    })
    expect(notifications.value[1].title).toBe('Other event')
    expect(unreadNotificationCount()).toBe(0)
  })

  it('refines unknown time in place and keeps that known time through partial replays', () => {
    receive(alarm({ ts: undefined }))
    const entryId = notifications.value[0].id
    expect(notifications.value[0].timestamp).toBeNull()
    markAllNotificationsRead()
    receive()
    receive(alarm({ ts: 'not-a-date' }))
    receive(alarm({ ts: '2026-11-17T10:47:00-08:00' }))
    expect(notifications.value).toHaveLength(1)
    expect(notifications.value[0]).toMatchObject({
      id: entryId,
      timestamp: Date.parse(timestamp),
      read: true,
    })
  })

  it('records another occurrence when a slot has a different genuine event timestamp', () => {
    receive()
    markAllNotificationsRead()
    receive(alarm({ ts: '2026-11-17T19:47:00Z' }))
    expect(notifications.value).toHaveLength(2)
    expect(notifications.value[0]).toMatchObject({
      timestamp: Date.parse('2026-11-17T19:47:00Z'),
      read: false,
      occurrence: { active: true },
    })
    expect(notifications.value[1]).toMatchObject({
      read: true,
      occurrence: { active: false },
    })
    expect(notifications.value[0].id).not.toBe(notifications.value[1].id)
  })

  it.each([timestamp, undefined])(
    'records clear then recurrence even with unchanged time %s',
    (ts) => {
      receive(alarm({ ts }))
      markAllNotificationsRead()
      endNotificationOccurrence(alarm().id)
      receive(alarm({ ts }))
      expect(notifications.value).toHaveLength(2)
      expect(notifications.value.map((entry) => entry.read)).toEqual([false, true])
    }
  )

  it('keeps equal text from different IDs and sources separate', () => {
    receive()
    receive(alarm({ id: 'victron-platform-0-2' }))
    receive(alarm({ source: 'another-source' }))
    expect(notifications.value).toHaveLength(3)
    expect(unreadNotificationCount()).toBe(3)
  })

  it('separates installations and applies a clear only to the current installation', () => {
    const notification = alarm()
    const time = Date.parse(timestamp)
    upsertNotification(notification, time, 'installation-a')
    upsertNotification(notification, time, 'installation-b')
    markAllNotificationsRead()
    endNotificationOccurrence(notification.id, 'installation-b')
    upsertNotification(notification, time, 'installation-a')
    expect(notifications.value).toHaveLength(2)
    expect(unreadNotificationCount()).toBe(0)
    upsertNotification(notification, time, 'installation-b')
    expect(notifications.value).toHaveLength(3)
    expect(unreadNotificationCount()).toBe(1)
  })

  it('does not treat banner dismissal or a transport banner reset as a source clear', async () => {
    receive()
    markAllNotificationsRead()
    await dismissBanner(alarm().id)
    clearVictronBanners()
    receive()
    expect(notifications.value).toHaveLength(1)
    expect(notifications.value[0].read).toBe(true)
  })

  it('keeps local notifications append-only even when text and time match', () => {
    addNotification('Local event', 'Same text', 123)
    markAllNotificationsRead()
    addNotification('Local event', 'Same text', 123)
    expect(notifications.value).toHaveLength(2)
    expect(notifications.value.map((entry) => entry.read)).toEqual([false, true])
  })

  it('bounds occurrence tracking with the retained history', () => {
    for (let index = 0; index < 120; index += 1) receive(alarm({ id: `alarm-${index}` }))
    expect(notifications.value).toHaveLength(100)
    expect(notifications.value[99].occurrence?.id).toBe('alarm-20')
    markAllNotificationsRead()
    receive(alarm({ id: 'alarm-119' }))
    expect(notifications.value).toHaveLength(100)
    expect(unreadNotificationCount()).toBe(0)
    clearNotifications()
    receive(alarm({ id: 'alarm-119' }))
    expect(notifications.value).toHaveLength(1)
    expect(unreadNotificationCount()).toBe(1)
  })
})
