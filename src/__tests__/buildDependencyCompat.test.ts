// @vitest-environment node
import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises'
import { createRequire } from 'node:module'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { expect, test } from 'vitest'

// Resolve through each real consumer so pnpm and npm exercise their installed
// dependency edges, including the targeted EditorConfig security override.
const require = createRequire(import.meta.url)
const requireTestUtils = createRequire(require.resolve('@vue/test-utils'))
const requireBeautify = createRequire(requireTestUtils.resolve('js-beautify'))
const beautify = requireTestUtils('js-beautify')
const editorconfig = requireBeautify('editorconfig')
const glob = requireBeautify('glob')

test('fixed glob dependencies retain formatting and EditorConfig matching', async () => {
  const root = await mkdtemp(join(tmpdir(), 'formatting-dependencies-'))
  try {
    await mkdir(join(root, 'nested'))
    await writeFile(
      join(root, '.editorconfig'),
      [
        'root = true',
        '[*.{ts,vue}]',
        'indent_style = space',
        'indent_size = 2',
        '[*.json]',
        'indent_size = 4',
        '',
      ].join('\n')
    )
    await writeFile(join(root, 'view.vue'), '<div><span>ok</span></div>')
    await writeFile(join(root, 'nested', 'logic.ts'), 'const ok = true')
    await writeFile(join(root, 'config.json'), '{}')
    expect(glob.globSync('**/*.{ts,vue}', { cwd: root, nodir: true }).sort()).toEqual([
      'nested/logic.ts',
      'view.vue',
    ])
    expect((await editorconfig.parse(join(root, 'nested', 'logic.ts'))).indent_size).toBe(2)
    expect((await editorconfig.parse(join(root, 'view.vue'))).indent_size).toBe(2)
    expect((await editorconfig.parse(join(root, 'config.json'))).indent_size).toBe(4)
    expect(beautify.html('<div><span>ok</span></div>', { indent_size: 2 })).toContain(
      '<span>ok</span>'
    )
  } finally {
    await rm(root, { recursive: true, force: true })
  }
})
