import { fileURLToPath, URL } from 'node:url';

import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import type { Plugin } from 'vite';
import { defineConfig } from 'vitest/config';

/**
 * Production Content-Security-Policy. Injected at build time only: the dev server relies on an
 * inline React Refresh preamble and HMR websockets that a strict policy would block.
 * The theme no-flash script lives in /public/theme-init.js so `script-src 'self'` suffices.
 */
const CONTENT_SECURITY_POLICY = [
  "default-src 'self'",
  "script-src 'self'",
  "style-src 'self' 'unsafe-inline'",
  "img-src 'self' data: blob:",
  "font-src 'self' data:",
  "connect-src 'self' ws: wss:",
  "object-src 'none'",
  "base-uri 'self'",
  "form-action 'self'",
].join('; ');

/** Primary UI font (latin subset) — preloaded so text renders without a late font swap. */
const PRELOAD_FONT = /geist-latin-wght-normal-[\w-]+\.woff2$/;

/** Build-only index.html additions: the CSP meta tag and a preload for the main font. */
function productionHtmlPlugin(): Plugin {
  return {
    name: 'nexc:production-html',
    apply: 'build',
    transformIndexHtml: {
      // `post` runs after bundling, when `ctx.bundle` (hashed asset names) is available.
      order: 'post',
      handler: (_html, ctx) => {
        const font = Object.keys(ctx.bundle ?? {}).find((file) => PRELOAD_FONT.test(file));
        return [
          {
            tag: 'meta',
            attrs: { 'http-equiv': 'Content-Security-Policy', content: CONTENT_SECURITY_POLICY },
            injectTo: 'head-prepend',
          },
          ...(font
            ? [
                {
                  tag: 'link',
                  attrs: {
                    rel: 'preload',
                    as: 'font',
                    type: 'font/woff2',
                    href: `/${font}`,
                    crossorigin: '',
                  },
                  injectTo: 'head' as const,
                },
              ]
            : []),
        ];
      },
    },
  };
}

export default defineConfig({
  plugins: [react(), tailwindcss(), productionHtmlPlugin()],
  // Identifies this build, so data cached in the browser by another build is not reused.
  define: { __NEXC_BUILD__: JSON.stringify(Date.now().toString(36)) },
  resolve: {
    alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) },
  },
  server: {
    port: 5173,
    strictPort: true,
    proxy: {
      // NEXC_API_URL points the dev proxy at a backend on another port.
      '/api': {
        target: process.env.NEXC_API_URL ?? 'http://localhost:8080',
        changeOrigin: true,
        ws: true,
      },
    },
  },
  preview: { port: 4173 },
  build: {
    target: 'es2023',
    sourcemap: true,
    rolldownOptions: {
      output: {
        codeSplitting: {
          groups: [
            {
              name: 'vendor-react',
              test: /node_modules[\\/](react|react-dom|react-router|react-router-dom|scheduler)[\\/]/,
              priority: 30,
            },
            {
              name: 'vendor-d3',
              test: /node_modules[\\/](d3|d3-[a-z-]+|internmap|delaunator|robust-predicates)[\\/]/,
              priority: 30,
            },
            {
              name: 'vendor-query',
              test: /node_modules[\\/](@tanstack|zod|zustand)[\\/]/,
              priority: 20,
            },
            {
              name: 'vendor-ui',
              test: /node_modules[\\/](radix-ui|@radix-ui|@floating-ui|cmdk|lucide-react|sonner|cn|class-variance-authority|react-remove-scroll[a-z-]*|vaul)[\\/]/,
              priority: 10,
            },
          ],
        },
      },
    },
  },
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: ['./src/test/setup.ts'],
    css: false,
    restoreMocks: true,
  },
});
