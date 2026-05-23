/** @type {import('tailwindcss').Config} */
export default {
  content: [
    './index.html',
    './src/**/*.{vue,js,ts,jsx,tsx}',
  ],
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        'apple-blue': '#007AFF',
        'apple-gray': '#8E8E93',
        'apple-gray-2': '#AEAEB2',
        'apple-gray-3': '#C7C7CC',
        'apple-gray-4': '#D1D1D6',
        'apple-gray-5': '#E5E5EA',
        'apple-gray-6': '#F2F2F7',
        'apple-background-light': '#F5F5F7',
        'apple-background-dark': '#1E1E1E',
        'apple-card-light': 'rgba(255, 255, 255, 0.8)',
        'apple-card-dark': 'rgba(44, 44, 46, 0.8)',
        'grid': '#2ecc71',
        'consumption': '#e74c3c',
        'solar': '#f1c40f',
        'battery': '#3498db',
      }
    }
  }
}
