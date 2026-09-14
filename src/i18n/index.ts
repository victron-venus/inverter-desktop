import { createI18n } from 'vue-i18n'
import en from './en'
import ru from './ru'
import featureMessages from '@feature-messages'

export const i18n = createI18n({
  legacy: false,
  locale: 'en',
  fallbackLocale: 'en',
  messages: { en, ru },
})

i18n.global.mergeLocaleMessage('en', featureMessages.en)
i18n.global.mergeLocaleMessage('ru', featureMessages.ru)

export default i18n
