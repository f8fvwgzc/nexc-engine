/**
 * What the pages do over time, against the mocked API: a list that follows a live run, a page
 * that recovers from a failed load, data kept on screen when a background refetch fails, a
 * filter that matches nothing, and the unread count in the tablet's icon rail.
 */
import { expect, test, type Page } from '@playwright/test';

import { fixtures } from './fixtures';
import { mockApi } from './support/mock-api';
import { openRoute } from './support/page';
import { APP_ROUTES, type AppRoute } from './support/routes';

const API = '/api/v1';

function route(name: string): AppRoute {
  const found = APP_ROUTES.find((r) => r.name === name);
  if (!found) throw new Error(`no route named ${name}`);
  return found;
}

/** Answers every call to a path matching `pattern` with a 500 from now on. */
async function failFromNow(page: Page, pattern: RegExp) {
  await page.route(
    (url) => url.pathname.startsWith(`${API}/`) && pattern.test(url.pathname.slice(API.length)),
    (r) =>
      r.fulfill({
        status: 500,
        json: { type: 'about:blank', title: 'Internal Server Error', status: 500, detail: null },
      }),
  );
}

/** The reader leaves the tab for two minutes and comes back: stale data is refetched. */
async function comeBackLater(page: Page) {
  await page.clock.fastForward('02:00');
  await page.evaluate(() => window.dispatchEvent(new Event('visibilitychange')));
}

test.describe('the Runs page', () => {
  test('shows a run that finished while the page stayed open', async ({ page }) => {
    const live = fixtures.runs.find((r) => r.status === 'running');
    if (!live) throw new Error('the fixtures need a running run');
    await page.setViewportSize({ width: 1280, height: 800 });
    await openRoute(page, route('runs'), { mode: 'full', theme: 'light' });

    const status = page.locator('tr', { hasText: live.id.slice(0, 8) }).getByRole('status');
    await expect(status).toHaveAccessibleName('Running');

    // The run finishes; nothing on this page holds the graph's event stream.
    const finished = { ...live, status: 'succeeded', finished_at: '2026-10-14T12:00:05Z' };
    await page.route(
      (url) => url.pathname === `${API}/graphs/${live.graph_id}/runs`,
      (r) =>
        r.fulfill({
          json: fixtures.runs
            .filter((run) => run.graph_id === live.graph_id)
            .map((run) => (run.id === live.id ? finished : run)),
        }),
    );
    await page.clock.fastForward('00:11');
    await expect(status).toHaveAccessibleName('Succeeded');
  });
});

test.describe('a page that failed to load', () => {
  for (const name of ['runs', 'agents']) {
    test(`${name}: Try again reads it again`, async ({ page }) => {
      await page.setViewportSize({ width: 1280, height: 800 });
      const target = route(name);
      await openRoute(page, target, { mode: 'error', theme: 'light' });
      await expect(page.getByRole('heading', { level: 1 })).toHaveText('Something went wrong');

      // The server is back.
      await page.unrouteAll({ behavior: 'ignoreErrors' });
      await mockApi(page, { mode: 'full', persona: target.persona });
      await page.getByRole('button', { name: 'Try again' }).click();

      await expect(page.getByRole('heading', { level: 1 })).not.toHaveText('Something went wrong');
      await expect(page.getByRole('button', { name: 'Try again' })).toHaveCount(0);
    });
  }
});

test.describe('a background refetch that fails', () => {
  test('keeps the issues on screen', async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    await openRoute(page, route('issues'), { mode: 'full', theme: 'light' });
    const title = fixtures.issues[0]?.title ?? '';
    await expect(page.getByText(title).first()).toBeVisible();

    await failFromNow(page, /^\/workspaces\/[^/]+\/issues$/);
    await comeBackLater(page);
    // The refetch failed (after its retries): the toast says so.
    await expect(page.locator('[data-sonner-toast]').first()).toBeVisible({ timeout: 15_000 });
    await expect(page.getByText('Couldn’t load the issues')).toHaveCount(0);
    await expect(page.getByText(title).first()).toBeVisible();
  });

  test('keeps the project on screen', async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    await openRoute(page, route('project'), { mode: 'full', theme: 'light' });
    const name = fixtures.projects[0]?.name ?? '';
    await expect(page.getByRole('heading', { level: 1 })).toHaveText(name);

    await failFromNow(page, /^\/workspaces\/[^/]+\/projects$/);
    await comeBackLater(page);
    await expect(page.locator('[data-sonner-toast]').first()).toBeVisible({ timeout: 15_000 });
    await expect(page.getByText('Couldn’t load the project')).toHaveCount(0);
    await expect(page.getByRole('heading', { level: 1 })).toHaveText(name);
  });
});

test('Sessions tells a filter that matches nothing from an empty workspace', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await openRoute(page, route('sessions'), { mode: 'full', theme: 'light' });
  await expect(page.getByRole('table')).toBeVisible();
  // The mock does not filter: a status filter matches nothing here.
  await page.route(
    (url) => /\/workspaces\/[^/]+\/sessions$/.test(url.pathname) && url.searchParams.has('status'),
    (r) => r.fulfill({ json: { sessions: [], next: null } }),
  );

  await page.getByRole('combobox', { name: 'Status' }).selectOption('failed');
  await expect(page.getByText('No matching sessions')).toBeVisible();
  await expect(page.getByText('No sessions yet')).toHaveCount(0);

  await page.getByRole('button', { name: 'Clear the filters' }).click();
  await expect(page.getByRole('combobox', { name: 'Status' })).toHaveValue('');
  await expect(page.getByRole('table')).toBeVisible();
});

test.describe('the inbox unread count', () => {
  const unread = fixtures.inbox.filter((n) => n.read_at === null).length;

  for (const width of [768, 1280]) {
    test(`is shown and named at ${width} px`, async ({ page }) => {
      await page.setViewportSize({ width, height: width === 768 ? 1024 : 800 });
      await openRoute(page, route('dashboard'), { mode: 'full', theme: 'light' });

      const inbox = page.getByRole('link', { name: `Inbox (${unread} unread)` });
      await expect(inbox).toBeVisible();
      if (width === 768) {
        // The rail has no room for the badge: a dot on the icon stands in for it.
        await expect(inbox.locator('[data-slot="unread-dot"]')).toBeVisible();
      } else {
        const row = page.getByRole('listitem').filter({ has: inbox });
        await expect(row.locator('[data-sidebar="menu-badge"]')).toHaveText(String(unread));
        await expect(inbox.locator('[data-slot="unread-dot"]')).toBeHidden();
      }
    });
  }
});
