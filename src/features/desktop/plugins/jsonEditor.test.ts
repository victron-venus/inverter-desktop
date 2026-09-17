import { reactive, readonly } from 'vue'
import { describe, expect, it } from 'vitest'
import { initialJsonValue } from './jsonEditor'

describe('JSON editor initial values', () => {
  for (const wrap of [reactive, readonly]) {
    for (const keyword of ['default', 'const'] as const) {
      it(`copies ${wrap.name} ${keyword} arrays and objects into independent drafts`, () => {
        const source = { entries: [{ label: 'Porch', enabled: false }], count: 0 }
        const schema = wrap({
          type: 'object' as const,
          properties: {
            controls: { type: 'array' as const, [keyword]: [source] },
            options: { type: 'object' as const, [keyword]: source },
          },
        })
        const first = initialJsonValue(schema) as {
          controls: (typeof source)[]
          options: typeof source
        }
        const second = initialJsonValue(schema)
        expect(first).toEqual({ controls: [source], options: source })
        expect(second).toEqual(first)
        expect(() => structuredClone(first)).not.toThrow()

        first.controls[0].entries[0].label = 'Changed'
        first.options.entries.push({ label: 'Another', enabled: true })
        expect(second).toEqual({ controls: [source], options: source })
        expect(source).toEqual({ entries: [{ label: 'Porch', enabled: false }], count: 0 })
      })
    }
  }
})
