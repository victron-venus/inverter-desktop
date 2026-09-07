import { createI18n } from 'vue-i18n'
import en from './en'
import ru from './ru'

export const i18n = createI18n({
  legacy: false,
  locale: 'en',
  fallbackLocale: 'en',
  messages: { en, ru },
})

export default i18n
