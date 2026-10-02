import { defineConfig } from 'vitest/config';

// LLM_PROXY_TARGET lets the dev server forward /llm-proxy/* to an endpoint
// that does not send CORS headers (set the base URL to /llm-proxy in settings).
const proxyTarget = process.env.LLM_PROXY_TARGET;
// The Rust server (server/), for /api and /auth during development.
const serverTarget = process.env.SERVER_URL ?? 'http://localhost:8080';

export default defineConfig({
  server: {
    port: 5173,
    strictPort: false,
    proxy: {
      // The event stream passes through unbuffered; the browser keeps the dev server's origin, so cookies work.
      '/api': { target: serverTarget },
      '/auth': { target: serverTarget },
      ...(proxyTarget
        ? {
            '/llm-proxy': {
              target: proxyTarget,
              changeOrigin: true,
              rewrite: (path: string) => path.replace(/^\/llm-proxy/, ''),
            },
          }
        : {}),
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
