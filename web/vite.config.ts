import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';
import { defineConfig } from 'vite';

// `npm run dev` serves the UI with hot reload and passes /api to a running `carrel web`. Open the link
// `carrel web` prints once, with this server's port instead of its own, to get the cookie.
export default defineConfig({
  plugins: [tailwindcss(), svelte()],
  server: {
    port: 5173,
    proxy: {
      '/api': {
        target: 'http://127.0.0.1:7700',
        changeOrigin: true,
        // carrel refuses writes from other origins; this one is the dev server standing in for it.
        configure: (proxy) => proxy.on('proxyReq', (req) => req.getHeader('origin') && req.setHeader('origin', 'http://127.0.0.1:7700')),
      },
    },
  },
  build: { outDir: 'dist', emptyOutDir: true },
});
