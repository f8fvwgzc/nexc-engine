import { fileURLToPath, URL } from 'node:url';

import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import type { Plugin } from 'vite';
import { defineConfig } from 'vitest/config';

/**
 * The origin the workspace map's frame may load: onto-intelli's UI at NEXC_ONTO_PUBLIC_URL, or
 * none. Only `scheme://host[:port]`, never a wildcard.
 */
function frameSource(url: string | undefined): string {
  if (!url) return "'none'";
  try {
    const { protocol, origin } = new URL(url);
    return protocol === 'http:' || protocol === 'https:' ? origin : "'none'";
  } catch {
    return "'none'";
  }
}

/**
 * Production Content-Security-Policy, sent as a header: by nginx in the image (frontend/
 * nginx.conf, the same directives, frame-src written at start from NEXC_ONTO_PUBLIC_URL) and by
 * `vite preview` (the UI suite). The build carries no meta policy: a meta policy cannot know
 * where onto-intelli is, and both would apply. The dev server sends none: it relies on an inline
 * React Refresh preamble and HMR websockets that a strict policy would block. The theme
 * no-flash script lives in /public/theme-init.js so `script-src 'self'` suffices.
 */
function contentSecurityPolicy(frameSrc: string): string {
  return [
    "default-src 'self'",
    "script-src 'self'",
    "style-src 'self' 'unsafe-inline'",
    "img-src 'self' data: blob:",
    "font-src 'self' data:",
    "connect-src 'self' ws: wss:",
    `frame-src ${frameSrc}`,
    "object-src 'none'",
    "base-uri 'self'",
    "form-action 'self'",
    "frame-ancestors 'none'",
  ].join('; ');
}

/** Primary UI font (latin subset) — preloaded so text renders without a late font swap. */
const PRELOAD_FONT = /geist-latin-wght-normal-[\w-]+\.woff2$/;

/** Build-only index.html addition: a preload for the main font. */
function productionHtmlPlugin(): Plugin {
  return {
    name: 'nexc:production-html',
    apply: 'build',
    transformIndexHtml: {
      // `post` runs after bundling, when `ctx.bundle` (hashed asset names) is available.
      order: 'post',
      handler: (_html, ctx) => {
        const font = Object.keys(ctx.bundle ?? {}).find((file) => PRELOAD_FONT.test(file));
        return font
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
          : [];
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
      // The A2A surface lives at the root, where the protocol expects it (CONTRACT §10).
      '/a2a': {
        target: process.env.NEXC_API_URL ?? 'http://localhost:8080',
        changeOrigin: true,
      },
      '/.well-known/agent.json': {
        target: process.env.NEXC_API_URL ?? 'http://localhost:8080',
        changeOrigin: true,
      },
    },
  },
  preview: {
    port: 4173,
    headers: {
      'Content-Security-Policy': contentSecurityPolicy(
        frameSource(process.env.NEXC_ONTO_PUBLIC_URL),
      ),
    },
  },
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
    // e2e/ is Playwright's (pnpm e2e), not vitest's.
    include: ['src/**/*.test.{ts,tsx}'],
    environment: 'jsdom',
    globals: true,
    setupFiles: ['./src/test/setup.ts'],
    css: false,
    restoreMocks: true,
  },
});
