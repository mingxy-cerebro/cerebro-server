import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@': new URL('./src', import.meta.url).pathname,
    },
  },
  server: {
    proxy: {
      '/v1': {
        target: 'https://www.mengxy.cc',
        changeOrigin: true,
        secure: true,
      },
      '/health': {
        target: 'https://www.mengxy.cc',
        changeOrigin: true,
        secure: true,
      },
    },
  },
})
