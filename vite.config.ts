import { defineConfig } from 'vitest/config';

// The server (the Rust server in server/, or the reference server in tools/), for /api, /auth and /ads.json during development.
const serverTarget = process.env.SERVER_URL ?? 'http://localhost:8080';

export default defineConfig({
  server: {
    port: 5173,
    strictPort: false,
    proxy: {
      // The event stream passes through unbuffered; the browser keeps the dev server's origin, so cookies work.
      '/api': {
        target: serverTarget,
        // When the server dies mid-stream, end the browser's side too, or EventSource never notices and never reconnects.
        configure: (proxy) => {
          proxy.on('proxyRes', (proxyRes, _req, res) => {
            proxyRes.on('close', () => {
              if (!res.writableEnded) res.destroy();
            });
          });
        },
      },
      '/auth': { target: serverTarget },
      '/ads.json': { target: serverTarget },
    },
  },
  build: {
    target: 'es2022',
    chunkSizeWarningLimit: 1500,
  },
  test: {
    environment: 'node',
    include: ['src/**/*.test.ts', 'tests/**/*.test.ts'],
  },
});
