import { createApp } from 'vue'
import App from './App.vue'
import Config from './Config.vue'
import About from './About.vue'

import './style.css'
import './styles.css'
import './button-overrides.css'

import vuetify from './plugins/vuetify'
import 'vuetify/styles'

const path = window.location.pathname
const isConfigWindow = path === '/config'
const isAboutWindow = path === '/about'
const rootComponent = isConfigWindow ? Config : isAboutWindow ? About : App

const app = createApp(rootComponent)
app.use(vuetify)
app.mount('#app')
