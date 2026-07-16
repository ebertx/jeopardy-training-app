import { sveltekit } from '@sveltejs/kit/vite';
import tailwindcss from '@tailwindcss/vite';
import { defineConfig } from 'vite';

export default defineConfig({
  plugins: [tailwindcss(), sveltekit()],
  server: {
    proxy: {
      // VITE_API_PROXY lets a dev session point at a mock or remote backend
      // (and dodge other projects squatting on localhost:3000).
      '/api': process.env.VITE_API_PROXY ?? 'http://127.0.0.1:3000',
    },
  },
});
