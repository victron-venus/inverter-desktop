import type { UserConfig } from '@commitlint/types'

const config: UserConfig = {
  extends: ['@commitlint/config-conventional'],
  rules: {
    // Allow longer subject lines for descriptive commit messages.
    'header-max-length': [2, 'always', 100],
    // Permit both imperative ("add feature") and imperative+noun ("feat(scope): ...")
    // subject-case is set to sentence-case OR lower-case.
    'subject-case': [2, 'never', ['sentence-case', 'start-case', 'pascal-case', 'upper-case']],
  },
}

export default config
