import { fileURLToPath } from 'node:url'
import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'

const api = process.env.BAWKBIT_API ?? 'http://127.0.0.1:8229'

export default defineConfig({
  plugins: [svelte()],
  resolve: {
    alias: { $lib: fileURLToPath(new URL('./src/lib', import.meta.url)) }
  },
  build: {
    // the server's content security policy allows fonts and images from itself only, not data: URIs
    assetsInlineLimit: 0,
    target: 'es2022'
  },
  server: {
    proxy: {
      // the server refuses writes whose Origin is another site, and to it the dev server is one
      '/api': { target: api, changeOrigin: true, headers: { origin: api } }
    }
  }
})
