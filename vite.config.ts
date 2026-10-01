import { defineConfig } from 'vitest/config';

// LLM_PROXY_TARGET lets the dev server forward /llm-proxy/* to an endpoint
// that does not send CORS headers (set the base URL to /llm-proxy in settings).
const proxyTarget = process.env.LLM_PROXY_TARGET;

export default defineConfig({
  server: {
    port: 5173,
    strictPort: false,
    proxy: proxyTarget
      ? {
          '/llm-proxy': {
            target: proxyTarget,
            changeOrigin: true,
            rewrite: (path) => path.replace(/^\/llm-proxy/, ''),
          },
        }
      : undefined,
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
