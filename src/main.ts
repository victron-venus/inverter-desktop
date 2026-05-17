import { createApp } from 'vue'
import App from './App.vue'

import './style.css'
import './styles.css'
import './button-overrides.css'

import vuetify from './plugins/vuetify'
import 'vuetify/styles'

const app = createApp(App)
app.use(vuetify)
app.mount('#app')