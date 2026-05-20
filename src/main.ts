import { createApp } from 'vue'
import App from './App.vue'
import Config from './Config.vue'

import './style.css'
import './styles.css'
import './button-overrides.css'

import vuetify from './plugins/vuetify'
import 'vuetify/styles'

const isConfigWindow = window.location.pathname === '/config'
const rootComponent = isConfigWindow ? Config : App

const app = createApp(rootComponent)
app.use(vuetify)
app.mount('#app')
