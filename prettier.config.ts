import type { Config } from 'prettier'

const config: Config = {
  // Match the established code style in src/
  singleQuote: true,
  semi: false,
  printWidth: 100,
  tabWidth: 2,
  trailingComma: 'es5',
  // Vue-specific: don't add extra indent inside <script> / <style> blocks
  vueIndentScriptAndStyle: false,
}

export default config
