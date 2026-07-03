const isDev = import.meta.env.DEV
const LOG_BUFFER_MAX = 200

interface LogEntry {
  level: 'error' | 'warn' | 'log'
  args: unknown[]
  ts: number
}

const buffer: LogEntry[] = []

function push(level: LogEntry['level'], args: unknown[]) {
  buffer.push({
    level,
    args: args.map((a) => {
      if (a instanceof Error) return a.stack || a.message
      if (typeof a === 'object')
        try {
          return JSON.stringify(a)
        } catch {
          return String(a)
        }
      return a
    }),
    ts: Date.now(),
  })
  if (buffer.length > LOG_BUFFER_MAX) buffer.shift()
}

export const logger = {
  error: (...args: unknown[]) => {
    if (isDev) console.error(...args)
    push('error', args)
  },
  warn: (...args: unknown[]) => {
    if (isDev) console.warn(...args)
    push('warn', args)
  },
  log: (...args: unknown[]) => {
    if (isDev) console.log(...args)
    push('log', args)
  },
  getBuffer: (): ReadonlyArray<LogEntry> => buffer,
  clearBuffer: () => {
    buffer.length = 0
  },
}
