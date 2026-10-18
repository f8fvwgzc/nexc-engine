/**
 * The UI suite (`pnpm e2e`, `make e2e-ui`, CI): every route of the router against the mocked API,
 * with realistic and with empty data. A page must render from the fixtures alone: every API call
 * it makes is answered, and it throws nothing.
 */
import { expect, test } from '@playwright/test';

import type { DataMode } from './support/mock-api';
import { openRoute } from './support/page';
import { APP_ROUTES, HEIGHT } from './support/routes';

const MODES: DataMode[] = ['full', 'empty'];

for (const route of APP_ROUTES) {
  for (const mode of MODES) {
    if (route.signedOut && mode === 'empty') continue;

    test(`${route.name} renders with ${mode} data`, async ({ page }) => {
      await page.setViewportSize({ width: 1280, height: HEIGHT[1280] });
      const errors: string[] = [];
      page.on('pageerror', (error) => errors.push(error.message));

      const log = await openRoute(page, route, { mode, theme: 'light' });

      expect(log.unhandled, 'API calls no fixture answers').toEqual([]);
      expect(errors, 'uncaught errors').toEqual([]);
    });
  }
}
