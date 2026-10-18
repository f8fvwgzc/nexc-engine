/**
 * The UI suite (`pnpm e2e`, `make e2e-ui`, CI): every route of the router against the mocked API,
 * with realistic and with empty data, at 375, 768 and 1280 px. On each page:
 *
 * - every API call it makes is answered by a fixture, it throws nothing, and no loading
 *   placeholder is left once it has settled;
 * - the layout check: nothing scrolls sideways (the document or the page's own scroller), and no
 *   two visible interactive elements overlap;
 * - axe finds no serious or critical violation (light, and dark at 1280 px);
 * - the main pages match their screenshot baseline (light), within the threshold of
 *   `playwright.config.ts`. Baselines are rendered on Linux (`make e2e-ui-update`).
 */
import { expect, test, type Page } from '@playwright/test';

import type { DataMode } from './support/mock-api';
import {
  checkLayout,
  fitViewportToContent,
  isSeriousOrCritical,
  openRoute,
  runAxe,
} from './support/page';
import { APP_ROUTES, HEIGHT, WIDTHS, type AppRoute } from './support/routes';

const MODES: DataMode[] = ['full', 'empty'];

/** Opens the route and checks what holds for every page in every theme and width. */
async function openAndCheck(
  page: Page,
  route: AppRoute,
  mode: DataMode,
  theme: 'light' | 'dark',
  width: number,
) {
  await page.setViewportSize({ width, height: HEIGHT[width as keyof typeof HEIGHT] });
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));

  const opened = await openRoute(page, route, { mode, theme });

  expect(opened.unhandled, 'API calls no fixture answers').toEqual([]);
  expect(errors, 'uncaught errors').toEqual([]);
  expect(opened.stuckLoading, 'loading placeholders left once settled').toBe(0);

  const layout = await checkLayout(page);
  expect(layout.docOverflow, 'the page scrolls sideways').toBe(0);
  expect(layout.mainOverflow, 'the page content scrolls sideways').toBe(0);
  expect(layout.overlaps, 'interactive elements overlap').toEqual([]);

  const serious = (await runAxe(page)).filter(isSeriousOrCritical);
  expect(serious, 'serious or critical axe violations').toEqual([]);
}

for (const route of APP_ROUTES) {
  for (const mode of MODES) {
    // A public page has no workspace data to empty.
    if (route.signedOut && mode === 'empty') continue;
    for (const width of WIDTHS) {
      test(`${route.name} · ${mode} data · ${width} px`, async ({ page }) => {
        await openAndCheck(page, route, mode, 'light', width);

        if (mode === 'full' && route.baseline) {
          // The whole page, not the first screen: the shell scrolls inside <main>.
          await fitViewportToContent(page, width, HEIGHT[width], 3000);
          const fitted = await checkLayout(page);
          expect(fitted.overlaps, 'interactive elements overlap').toEqual([]);
          await expect(page).toHaveScreenshot(`${route.name}-${width}.png`, { fullPage: true });
        }
      });
    }
  }

  test(`${route.name} · full data · dark · 1280 px`, async ({ page }) => {
    await openAndCheck(page, route, 'full', 'dark', 1280);
  });
}
