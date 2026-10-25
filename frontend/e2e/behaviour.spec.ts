/**
 * What the pages do over time, against the mocked API: a list that follows a live run, a page
 * that recovers from a failed load, data kept on screen when a background refetch fails, an
 * issue that opens on its own page (and from an old `?issue=` link), the issue board's full
 * height, per-column `+` and folded columns, a filter that matches nothing, one team's projects
 * and a card dragged across the board, the unread count in the tablet's icon rail, and the
 * workspace map's embedded onto-intelli UI: framed under the console's own
 * Content-Security-Policy, signed in, and its session renewed by its expiry.
 */
import { expect, test, type Page } from '@playwright/test';

import { fixtures } from './fixtures';
import { mockApi } from './support/mock-api';
import { NOW, openRoute } from './support/page';
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
  // The route's error page stands in for a page without a boundary of its own (runs); a page
  // that has one (agents) keeps its header and shows the error under it.
  for (const { name, heading } of [
    { name: 'runs', heading: 'Something went wrong' },
    { name: 'agents', heading: 'Agents' },
  ]) {
    test(`${name}: Try again reads it again`, async ({ page }) => {
      await page.setViewportSize({ width: 1280, height: 800 });
      const target = route(name);
      await openRoute(page, target, { mode: 'error', theme: 'light' });
      await expect(page.getByRole('heading', { level: 1 })).toHaveText(heading);
      await expect(page.getByRole('alert').filter({ hasText: 'Try again' })).toBeVisible();

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

test.describe('an issue', () => {
  const issue = fixtures.issues[0];
  if (!issue) throw new Error('the fixtures need an issue');

  test('opens on its own page and Escape goes back to the list', async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    await openRoute(page, route('issues'), { mode: 'full', theme: 'light' });

    await page.locator('a[data-row-link]', { hasText: issue.title }).first().click();
    await expect(page).toHaveURL(new RegExp(`/app/issues/${issue.id}$`));
    await expect(page.getByRole('heading', { level: 1 })).toHaveText(
      `${issue.identifier} ${issue.title}`,
    );
    await expect(page.getByRole('textbox', { name: 'Title' })).toHaveValue(issue.title);
    await expect(page.getByRole('complementary', { name: 'Properties' })).toBeVisible();

    await page.keyboard.press('Escape');
    await expect(page).toHaveURL(/\/app\/issues$/);
    await expect(page.getByText(issue.title).first()).toBeVisible();
  });

  test('opened from an old link (`?issue=`) lands on its page, with no list to go back to', async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    await openRoute(
      page,
      { name: 'old-issue-link', path: `/app/issues?issue=${issue.id}`, persona: 'member' },
      { mode: 'full', theme: 'light' },
    );
    const here = new RegExp(`/app/issues/${issue.id}$`);
    await expect(page).toHaveURL(here);
    await expect(page.getByRole('textbox', { name: 'Title' })).toHaveValue(issue.title);

    // The redirect is the visit's first page: Escape stays (going back would leave the app).
    await page.keyboard.press('Escape');
    await expect(page).toHaveURL(here);

    // Deleted, it goes to the issues list.
    await page.route(
      (url) => url.pathname === `${API}/issues/${issue.id}`,
      (r) => (r.request().method() === 'DELETE' ? r.fulfill({ status: 204 }) : r.fallback()),
    );
    await page.getByRole('button', { name: `Delete ${issue.identifier}` }).click();
    await page.getByRole('button', { name: 'Delete issue' }).click();
    await expect(page).toHaveURL(/\/app\/issues$/);
  });
});

test('the issue board fills the page, files into a column and folds one away', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await openRoute(page, route('issues'), { mode: 'full', theme: 'light' });
  await page.getByRole('button', { name: 'Board view' }).click();

  // The columns reach down to the page's bottom gutter, whatever they hold.
  const backlog = page.getByRole('region', { name: 'Backlog' });
  // boundingBox() does not wait, so the bottom edge is read until the board has laid out.
  await expect
    .poll(async () => {
      const box = await backlog.boundingBox();
      return (box?.y ?? 0) + (box?.height ?? 0);
    })
    .toBeGreaterThan(800 - 48);

  await page.getByRole('button', { name: 'New issue in Todo' }).click();
  const dialog = page.getByRole('dialog', { name: 'New issue' });
  await expect(dialog.getByRole('combobox', { name: 'Status' })).toHaveText(/Todo/);
  await page.keyboard.press('Escape');
  await expect(dialog).toBeHidden();

  await page.getByRole('button', { name: 'Fold Backlog' }).click();
  await expect(backlog.getByRole('button', { name: /^Unfold Backlog/ })).toBeVisible();
  // Remembered for the team.
  await page.reload();
  await expect(backlog.getByRole('button', { name: /^Unfold Backlog/ })).toBeVisible();
  await backlog.getByRole('button', { name: /^Unfold Backlog/ }).click();
  await expect(page.getByRole('button', { name: 'Fold Backlog' })).toBeVisible();

  // From the keyboard the focus stays on the column's button as it folds and unfolds.
  await page.getByRole('button', { name: 'Fold Backlog' }).focus();
  await page.keyboard.press('Enter');
  await expect(backlog.getByRole('button', { name: /^Unfold Backlog/ })).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(page.getByRole('button', { name: 'Fold Backlog' })).toBeFocused();
});

test.describe('the Projects page', () => {
  test('lists one team’s projects and moves a card to another status', async ({ page }) => {
    const project = fixtures.projects.find((p) => p.team?.key === 'ENG');
    if (!project?.team_id) throw new Error('the fixtures need a project owned by ENG');
    await page.setViewportSize({ width: 1280, height: 800 });
    await openRoute(
      page,
      { ...route('projects'), path: `/app/projects?team=${project.team_id}` },
      { mode: 'full', theme: 'light' },
    );

    // The list is asked for the team's projects only (the mock filters by team_id).
    const main = page.getByRole('main');
    await expect(main.getByRole('link', { name: project.name })).toBeVisible();
    for (const other of fixtures.projects.filter((p) => p.id !== project.id)) {
      await expect(main.getByText(other.name)).toHaveCount(0);
    }

    // The server keeps the move: the list read again after it has the project paused.
    let moved = false;
    await page.route(
      (url) => url.pathname.endsWith(`/projects/${project.id}`),
      (r) => {
        if (r.request().method() !== 'PATCH') return r.fallback();
        moved = true;
        return r.fulfill({ json: { ...project, status: 'paused' } });
      },
    );
    await page.route(
      (url) => /\/workspaces\/[^/]+\/projects$/.test(url.pathname),
      (r) => (moved ? r.fulfill({ json: [{ ...project, status: 'paused' }] }) : r.fallback()),
    );

    await page.getByRole('button', { name: 'Board view' }).click();
    const paused = main.getByRole('region', { name: 'Paused' });
    const patch = page.waitForRequest(
      (r) => r.method() === 'PATCH' && r.url().endsWith(`/projects/${project.id}`),
    );
    await main.getByRole('link', { name: project.name }).dragTo(paused);
    expect((await patch).postDataJSON()).toEqual({ status: 'paused' });
    await expect(paused.getByRole('link', { name: project.name })).toBeVisible();
    await expect(
      main.getByRole('region', { name: 'In progress' }).getByRole('link', { name: project.name }),
    ).toHaveCount(0);
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
      await openRoute(page, route('inbox'), { mode: 'full', theme: 'light' });

      // The rail's Issues icon carries a dot and the count in its name at every width.
      const issues = page.getByRole('link', { name: `Issues (${unread} unread)` });
      await expect(issues).toBeVisible();
      await expect(issues.locator('[data-slot="unread-dot"]')).toBeVisible();
      const inbox = page.getByRole('link', { name: `Inbox (${unread} unread)` });
      if (width === 768) {
        // On a tablet the panel is closed to the rail: the Inbox row is not on screen.
        await expect(inbox).toBeHidden();
      } else {
        await expect(inbox).toBeVisible();
        const row = page.getByRole('listitem').filter({ has: inbox });
        await expect(row.locator('[data-sidebar="menu-badge"]')).toHaveText(String(unread));
      }
    });
  }
});

test.describe('the workspace map', () => {
  test('frames onto-intelli’s UI and hands it the member’s session', async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    await openRoute(page, route('settings-map'), { mode: 'full', theme: 'light' });
    // The build is served with the console's Content-Security-Policy (vite preview, as nginx):
    // its frame-src lets the stand-in onto-intelli load, which shows what it was handed.
    const frame = page.frameLocator('iframe[title^="Workspace map"]');
    await expect(
      frame.getByText(`Signed in to ${fixtures.ontologySession.workspace}`),
    ).toBeVisible();
  });

  test('renews the session by its own expiry', async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.clock.install({ time: NOW });
    await mockApi(page, { mode: 'full', persona: 'member' });
    // Sessions of ten minutes (NEXC_ONTO_SESSION_TTL_SECS=600), each from when it is minted.
    let minted = 0;
    await page.route(
      (url) => url.pathname.endsWith('/ontology/session'),
      (r) => {
        minted += 1;
        const expires = NOW.getTime() + (minted === 1 ? 10 : 18) * 60_000;
        return r.fulfill({
          json: { ...fixtures.ontologySession, expires_at: new Date(expires).toISOString() },
        });
      },
    );
    await page.goto(route('settings-map').path);
    await expect.poll(() => minted).toBe(1);
    await page.clock.fastForward('06:00');
    expect(minted).toBe(1);
    // Three quarters of its ten minutes: a new one, before the frame's runs out.
    await page.clock.fastForward('02:00');
    await expect.poll(() => minted).toBe(2);
  });
});
