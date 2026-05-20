import { createVuetify } from 'vuetify'
import * as components from 'vuetify/components'
import * as directives from 'vuetify/directives'
import '@mdi/font/css/materialdesignicons.css'

export default createVuetify({
  components,
  directives,
  theme: {
    defaultTheme: 'light',
    themes: {
      light: {
        dark: false,
        colors: {
          primary: '#4CAF50',
          secondary: '#2196F3',
          success: '#4CAF50',
          warning: '#FF9800',
          error: '#F44336',
          info: '#2196F3'
        }
      },
      dark: {
        dark: true,
        colors: {
          primary: '#66BB6A',
          secondary: '#42A5F5',
          success: '#66BB6A',
          warning: '#FFA726',
          error: '#EF5350',
          info: '#42A5F5',
          background: '#121212',
          surface: '#1E1E1E'
        }
      }
    }
  }
})
