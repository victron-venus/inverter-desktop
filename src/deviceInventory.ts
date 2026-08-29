/** Add-only device inventory for Batteries / Solar Production tiles. */

export const DISCOVERY_INTERVAL_MS = 5 * 60 * 1000

export interface InventoryDevice {
  name?: string
  serial?: string
  instance?: number
}

const FALLBACK_NAME = /^(mppt|battery|pv inverter|solar charger)(?:[- ](\d+))?$/i
const PRODUCT_PREFIX = /^(smartsolar|bluesolar|smartshunt|multiplus|phoenix|victron|mppt)\b/i

function norm(s?: string | null): string {
  return (s ?? '').trim()
}

function isFallbackName(name?: string | null): boolean {
  return FALLBACK_NAME.test(norm(name))
}

function isProductName(name?: string | null): boolean {
  const n = norm(name)
  if (!n || isFallbackName(n)) return false
  return PRODUCT_PREFIX.test(n)
}

/** Higher rank wins. CustomName (4) beats ProductName (3) beats `MPPT-N` (2) beats bare `MPPT` (1). */
export function nameRank(name?: string | null): number {
  const n = norm(name)
  if (!n) return 0
  if (isFallbackName(n)) return /[- ]\d+$/.test(n) ? 2 : 1
  if (isProductName(n)) return 3
  return 4
}

/** CustomName (rank 4) wins. Within same rank, prefer longer / newer name. */
export function lockName(current?: string | null, incoming?: string | null): string | undefined {
  const cur = norm(current)
  const inc = norm(incoming)
  if (!inc) return current ?? undefined
  if (!cur) return incoming ?? inc
  const cr = nameRank(cur)
  const ir = nameRank(inc)
  if (ir > cr) return incoming ?? inc
  if (ir < cr) return current ?? cur
  // Same rank: prefer incoming (last-write-wins), tiebreak on length.
  if (inc.length >= cur.length) return incoming ?? inc
  return current ?? cur
}

export function shouldDiscover(lastDiscoveryMs: number | null, nowMs: number): boolean {
  if (lastDiscoveryMs == null) return true
  return nowMs - lastDiscoveryMs >= DISCOVERY_INTERVAL_MS
}

/**
 * Union two snapshots: keep every previously seen device, refresh metrics for
 * matches, lock names, and (when `addNew`) append first-seen devices.
 * Identity is serial, then Venus instance, then exact name.
 */
export function mergeDeviceInventory<T extends InventoryDevice>(
  existing: T[] | undefined,
  incoming: T[] | undefined,
  options?: { addNew?: boolean }
): T[] {
  const addNew = options?.addNew !== false
  const dest = existing ? existing.map((d) => ({ ...d })) : []
  if (!incoming) return dest
  if (dest.length === 0) return incoming.map((d) => ({ ...d }))

  for (let i = 0; i < incoming.length; i++) {
    const inc = incoming[i]
    let matchIdx = -1
    for (let j = 0; j < dest.length; j++) {
      const dst = dest[j]
      if (
        (dst.serial && inc.serial && dst.serial === inc.serial) ||
        (dst.instance === inc.instance && inc.instance != null && Number.isFinite(inc.instance))
      ) {
        matchIdx = j
        break
      }
    }
    if (matchIdx >= 0) {
      const cur = dest[matchIdx]
      dest[matchIdx] = {
        ...cur,
        ...inc,
        name: lockName(cur.name, inc.name),
      }
      continue
    }
    if (!addNew) continue
    // Fall back to exact-name dedupe (e.g. legacy entries without serial/instance).
    const nameIdx = dest.findIndex((d) => norm(d.name) && norm(d.name) === norm(inc.name))
    if (nameIdx >= 0) {
      const cur = dest[nameIdx]
      dest[nameIdx] = { ...cur, ...inc, name: lockName(cur.name, inc.name) }
      continue
    }
    dest.push({ ...inc })
  }
  return dest
}
