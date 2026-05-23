import { createApp } from 'vue'
import App from './App.vue'
import Config from './Config.vue'
import About from './About.vue'

import './style.css'

const path = globalThis.location.pathname
const isConfigWindow = path === '/config'
const isAboutWindow = path === '/about'

let rootComponent
if (isConfigWindow) {
  rootComponent = Config
} else if (isAboutWindow) {
  rootComponent = About
} else {
  rootComponent = App
}

const app = createApp(rootComponent)
app.mount('#app')
