import { createApp, h, type Component } from 'vue'
import AuthGate from './components/AuthGate.vue'
import About from './About.vue'
import App from './App.vue'
import { getFeatureView, isMobileApp } from '@features'
import MobileShell from './MobileShell.vue'
import Config from './Config.vue'
import { i18n } from './i18n'

import { logger } from './logger'
import './style.css'

document.documentElement.dataset.appProfile = isMobileApp ? 'mobile' : 'desktop'

const path = globalThis.location.pathname
const isConfigWindow = path === '/config'
const isAboutWindow = path === '/about'

let rootComponent: Component
if (isMobileApp) {
  rootComponent = MobileShell
} else if (isConfigWindow) {
  rootComponent = Config
} else if (isAboutWindow) {
  rootComponent = About
} else {
  rootComponent = getFeatureView(path) ?? App
}

const app = createApp({ render: () => h(AuthGate, null, { default: () => h(rootComponent) }) })
app.use(i18n)
app.config.errorHandler = (err, instance, info) => {
  logger.error('Unhandled Vue error:', err, 'Component:', instance, 'Info:', info)
}
app.mount('#app')
