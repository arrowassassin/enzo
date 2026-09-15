import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// https://vite.dev/config/
// The site is served from https://arrowassassin.github.io/quire/, so every asset
// path has to be resolved against this base (see `import.meta.env.BASE_URL`).
export default defineConfig({
  base: '/quire/',
  plugins: [react()],
  build: {
    target: 'es2022',
    assetsInlineLimit: 2048,
  },
})
