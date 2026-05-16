import { createApp } from 'vue'
import './style.css'
import App from './App.vue'
import VueAwesomeButton from 'vue-awesome-button'

const app = createApp(App)
app.component('VueAwesomeButton', VueAwesomeButton)
app.mount('#app')