import { createApp } from 'vue'
import About from './About.vue'
import App from './App.vue'
import Config from './Config.vue'
import { i18n } from './i18n'

import './style.css'

const path = globalThis.location.pathname
const isConfigWindow = path === '/config'
const isAboutWindow = path === '/about'

let rootComponent: typeof App | typeof Config | typeof About
if (isConfigWindow) {
  rootComponent = Config
} else if (isAboutWindow) {
  rootComponent = About
} else {
  rootComponent = App
}

const app = createApp(rootComponent)
app.use(i18n)
app.mount('#app')
