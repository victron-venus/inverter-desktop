import { readonly, shallowRef } from 'vue'

/** Identity supplied by the frontend and echoed only by the native client. */
export type TransportEvent<T> = T & { notification_session?: string }

let reservedSession: string | null = null
const activeSession = shallowRef<string | null>(null)
export const activeTransportSession = readonly(activeSession)

export function reserveTransportSession(): string {
  const token =
    typeof globalThis.crypto.randomUUID === 'function'
      ? globalThis.crypto.randomUUID()
      : Array.from(globalThis.crypto.getRandomValues(new Uint8Array(16)), (byte) =>
          byte.toString(16).padStart(2, '0')
        ).join('')
  reservedSession = token
  activeSession.value = null
  return token
}

export function isCurrentTransportSession(token: string | null): boolean {
  return token !== null && token === reservedSession
}

export function activateTransportSession(token: string) {
  if (isCurrentTransportSession(token)) activeSession.value = token
}

export function invalidateTransportSession(token: string | null) {
  if (!isCurrentTransportSession(token)) return
  reservedSession = null
  activeSession.value = null
}

export function acceptsCurrentTransportEvent(payload: unknown): boolean {
  return (
    activeSession.value !== null &&
    payload !== null &&
    typeof payload === 'object' &&
    'notification_session' in payload &&
    payload.notification_session === activeSession.value
  )
}

export function deactivateTransportSession(token: string) {
  if (isCurrentTransportSession(token)) activeSession.value = null
}

export function getCurrentTransportSession(): string | null {
  return activeSession.value
}
