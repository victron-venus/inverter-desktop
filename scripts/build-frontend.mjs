import { spawnSync } from 'node:child_process'
import { createRequire } from 'node:module'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { resolveFrontendProfile } from './frontend-profile.mjs'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const require = createRequire(import.meta.url)
const args = process.argv.slice(2)
if (args.length > 1 || (args[0] && !['mobile', 'desktop'].includes(args[0]))) {
  throw new Error('Usage: node scripts/build-frontend.mjs [mobile|desktop]')
}
const env = { ...process.env }
if (args[0]) env.INVERTER_BUILD_PROFILE = args[0]
const profile = resolveFrontendProfile(env)
env.INVERTER_BUILD_PROFILE = profile
console.log(`Building ${profile} frontend`)

// Invoke Node entrypoints directly: no shell or platform-specific .cmd quoting.
for (const [pkg, bin, flags] of [
  [
    'vue-tsc',
    'bin/vue-tsc.js',
    ['--noEmit', '--project', profile === 'mobile' ? 'tsconfig.mobile.json' : 'tsconfig.json'],
  ],
  ['vite', 'bin/vite.js', ['build']],
]) {
  const entry = path.join(path.dirname(require.resolve(`${pkg}/package.json`)), bin)
  const result = spawnSync(process.execPath, [entry, ...flags], {
    cwd: root,
    env,
    stdio: 'inherit',
  })
  if (result.error) throw result.error
  if (result.status !== 0) process.exit(result.status ?? 1)
}
