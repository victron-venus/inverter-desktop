import { createApp } from 'vue'
import './style.css'
import App from './App.vue'
import VueAwesomeButton from 'vue-awesome-button'
import 'vue-awesome-button/dist/style.css'

const app = createApp(App)
app.component('VueAwesomeButton', VueAwesomeButton)
app.mount('#app')