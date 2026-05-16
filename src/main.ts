import { createApp } from 'vue'
import './style.css'
import './styles.css'
import './button-overrides.css'
import App from './App.vue'
import { AwesomeButton } from 'vue-awesome-button'

const app = createApp(App)
app.component('VueAwesomeButton', AwesomeButton)
app.mount('#app')