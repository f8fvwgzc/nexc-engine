/**
 * The UI audit (`pnpm e2e:audit`): every route at 375, 768 and 1280 px, light and dark, with
 * realistic and with empty data. For each it saves a full-page screenshot and a JSON record of the
 * automated checks (axe, layout, labels, truncation, headers, empty states, focus, caching) under
 * `e2e/.audit/<phase>/`; `summary.spec.ts` folds the records into `summary.json`. It is a
 * tool for looking, not a gate: the gate is `ui.spec.ts`.
 */
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { test, type Page } from '@playwright/test';

import type { DataMode } from '../support/mock-api';
import {
  checkLayout,
  fitViewportToContent,
  openRoute,
  runAxe,
  waitForSettled,
} from '../support/page';
import { APP_ROUTES, HEIGHT, WIDTHS } from '../support/routes';

const PHASE = process.env.AUDIT_PHASE ?? 'current';
const AUDIT_DIR = join(dirname(fileURLToPath(import.meta.url)), '..', '.audit', PHASE);
const ONLY = process.env.AUDIT_ROUTES?.split(',');

const THEMES = ['light', 'dark'] as const;
const MODES: DataMode[] = ['full', 'empty'];

/** Visible controls without visible text, and whether anything names them on hover. */
async function iconOnlyControls(page: Page) {
  return page.evaluate(() =>
    // Switches and checkboxes are labelled by the text beside them; canvas nodes draw theirs.
    [...document.querySelectorAll('button, a[href], [role="button"]')]
      .filter((el) => !['switch', 'checkbox'].includes(el.getAttribute('role') ?? ''))
      .filter((el) => !el.closest('svg, .graph-canvas'))
      .filter((el) => {
        const r = el.getBoundingClientRect();
        if (r.width < 2 || r.height < 2 || el.closest('[aria-hidden="true"]')) return false;
        return ((el as HTMLElement).innerText ?? '').trim() === '';
      })
      .map((el) => ({
        label:
          el.getAttribute('aria-label') ??
          (el as HTMLButtonElement).labels?.[0]?.innerText ??
          document.getElementById(el.getAttribute('aria-labelledby') ?? '')?.innerText ??
          el.getAttribute('title') ??
          '',
        title: el.hasAttribute('title'),
        // A Radix tooltip trigger is marked as one, or carries data-state but no popup of its own.
        tooltip:
          el.getAttribute('data-slot') === 'tooltip-trigger' ||
          (el.hasAttribute('data-state') &&
            !el.hasAttribute('aria-haspopup') &&
            !el.hasAttribute('aria-expanded') &&
            !el.hasAttribute('role')),
        menu: el.hasAttribute('aria-haspopup'),
        html: el.outerHTML.slice(0, 160),
      })),
  );
}

/** Text cut with an ellipsis (or a line clamp) that nothing reveals in full. */
async function silentTruncation(page: Page) {
  return page.evaluate(() => {
    const found: string[] = [];
    for (const el of document.querySelectorAll<HTMLElement>('body *')) {
      // A field shows its whole value while it is edited.
      if (el.children.length > 3 || el instanceof HTMLInputElement) continue;
      const style = getComputedStyle(el);
      const clamped = style.webkitLineClamp !== 'none' && style.webkitLineClamp !== '';
      const ellipsis = style.textOverflow === 'ellipsis';
      if (!clamped && !ellipsis) continue;
      const cut = ellipsis
        ? el.scrollWidth > el.clientWidth + 1
        : el.scrollHeight > el.clientHeight + 1;
      if (!cut) continue;
      const named = el.closest('[title], [data-state], [aria-describedby]');
      if (!named) found.push(`${el.tagName.toLowerCase()}: ${el.innerText.slice(0, 60)}`);
    }
    return found;
  });
}

/** The page's headings, and where its title sits against the content. */
async function headings(page: Page) {
  return page.evaluate(() => {
    const scroller = document.querySelector('#main > .overflow-y-auto, #main .overflow-y-auto');
    const left = scroller?.getBoundingClientRect().left ?? 0;
    return [...document.querySelectorAll('h1')].map((h) => {
      const style = getComputedStyle(h);
      return {
        text: h.innerText.slice(0, 60),
        inset: Math.round(h.getBoundingClientRect().left - left),
        top: Math.round(h.getBoundingClientRect().top),
        size: style.fontSize,
        weight: style.fontWeight,
      };
    });
  });
}

/** How the page guides a reader who has nothing yet. */
async function emptyStates(page: Page) {
  return page.evaluate(() =>
    [...document.querySelectorAll('[data-slot="empty"]')].map((el) => ({
      title: el.querySelector('[data-slot="empty-title"]')?.textContent ?? '',
      icon: !!el.querySelector('svg'),
      description: !!el.querySelector('[data-slot="empty-description"]'),
      action: !!el.querySelector('button, a[href]'),
    })),
  );
}

/** Tabs through the page; controls whose focused look equals their resting look. */
async function invisibleFocus(page: Page) {
  const missing: string[] = [];
  await page
    .locator('body')
    .click({ position: { x: 1, y: 1 } })
    .catch(() => undefined);
  // The look of the focused control and of its parent (a card that rings when its link has
  // focus counts), measured a frame after each change so transitions have ended.
  const look = () =>
    page.evaluate(() => {
      const el = document.querySelector('[data-audit-focus]');
      if (!el) return '';
      // Only what can be seen: an outline whose style is none or whose colour is transparent,
      // or a shadow of transparent layers, draws nothing whatever its width.
      const transparent = /rgba\(0, 0, 0, 0\)|transparent/;
      const styles = (node: Element | null) => {
        if (!node) return '';
        const s = getComputedStyle(node);
        const outline =
          s.outlineStyle === 'none' || s.outlineWidth === '0px' || transparent.test(s.outlineColor)
            ? 'none'
            : `${s.outlineStyle} ${s.outlineWidth} ${s.outlineColor}`;
        const shadow = s.boxShadow
          .split(/,(?![^(]*\))/)
          .filter((layer) => layer.trim() !== 'none' && !transparent.test(layer))
          .join(',');
        return [outline, shadow, s.borderColor, s.backgroundColor, s.textDecorationLine].join('|');
      };
      return `${styles(el)}#${styles(el.parentElement)}`;
    });
  const settle = () => page.waitForTimeout(40);
  for (let i = 0; i < 30; i++) {
    await page.keyboard.press('Tab');
    const name = await page.evaluate(() => {
      document.querySelector('[data-audit-focus]')?.removeAttribute('data-audit-focus');
      const el = document.activeElement as HTMLElement | null;
      if (!el || el === document.body) return null;
      el.setAttribute('data-audit-focus', '');
      return `${el.tagName.toLowerCase()}[${el.getAttribute('aria-label') ?? el.innerText?.trim().slice(0, 30) ?? ''}]`;
    });
    if (!name) continue;
    await settle();
    const focused = await look();
    await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
    await settle();
    const rest = await look();
    // Back where the keyboard was, so the next Tab moves on from it.
    await page.evaluate(() => document.querySelector<HTMLElement>('[data-audit-focus]')?.focus());
    if (focused === rest) missing.push(name);
  }
  return [...new Set(missing)];
}

for (const route of APP_ROUTES) {
  if (ONLY && !ONLY.includes(route.name)) continue;
  for (const mode of MODES) {
    // A public page has no workspace data to empty.
    if (route.signedOut && mode === 'empty') continue;
    for (const theme of THEMES) {
      for (const width of WIDTHS) {
        test(`${route.name} ${mode} ${theme} ${width}`, async ({ page }) => {
          await page.setViewportSize({ width, height: HEIGHT[width] });
          const errors: string[] = [];
          page.on('pageerror', (e) => errors.push(e.message));
          page.on('console', (m) => {
            if (m.type() === 'error') errors.push(m.text().slice(0, 200));
          });

          const log = await openRoute(page, route, { mode, theme });
          const loadRequests = log.requests.map((r) => `${r.method} ${r.path}`);
          const record: Record<string, unknown> = {
            route: route.name,
            path: route.path,
            mode,
            theme,
            width,
            unhandled: log.unhandled,
            stuckLoading: log.stuckLoading,
            requests: loadRequests.length,
            duplicates: loadRequests.filter((r, i) => loadRequests.indexOf(r) !== i),
          };

          const primary = theme === 'light' && (width === 1280 || width === 375);
          if (primary) {
            record.iconOnly = await iconOnlyControls(page);
            record.truncation = await silentTruncation(page);
            record.headings = await headings(page);
            record.empty = await emptyStates(page);
          }
          record.layout = await checkLayout(page);
          record.axe = await runAxe(page);

          await fitViewportToContent(page, width, HEIGHT[width]);
          const shot = join(AUDIT_DIR, 'shots', mode, theme, String(width), `${route.name}.png`);
          mkdirSync(dirname(shot), { recursive: true });
          await page.screenshot({ path: shot, fullPage: true, animations: 'disabled' });
          record.fullLayout = await checkLayout(page);

          if (primary && width === 1280 && mode === 'full') {
            await page.setViewportSize({ width, height: HEIGHT[width] });
            await waitForSettled(page, 2_000);
            record.focus = await invisibleFocus(page);
            // Two minutes pass (polls fire), then the reader comes back to the tab: what is
            // fetched again because of the focus alone?
            const before = log.requests.length;
            await page.clock.fastForward('02:00');
            await page.waitForTimeout(300);
            record.polls = log.requests.slice(before).map((r) => `${r.method} ${r.path}`);
            const back = log.requests.length;
            await page.evaluate(() => window.dispatchEvent(new Event('visibilitychange')));
            await page.waitForTimeout(500);
            record.refetchOnFocus = log.requests.slice(back).map((r) => `${r.method} ${r.path}`);
          }
          record.errors = errors;

          const out = join(AUDIT_DIR, 'records', `${route.name}.${mode}.${theme}.${width}.json`);
          mkdirSync(dirname(out), { recursive: true });
          writeFileSync(out, JSON.stringify(record, null, 2));
        });
      }
    }
  }
}

/** What a page shows while its data is on the way, and when it cannot be had. */
async function stateRecord(page: Page) {
  return page.evaluate(() => ({
    skeletons: document.querySelectorAll('[data-slot="skeleton"]').length,
    busy: document.querySelectorAll('[aria-busy="true"]').length,
    spinners: document.querySelectorAll('[data-slot="spinner"]').length,
    loadingText: /Loading/i.test(document.querySelector('#main')?.textContent ?? ''),
    alerts: [...document.querySelectorAll('[role="alert"]')].map((a) =>
      a.textContent?.slice(0, 80),
    ),
    emptyStates: [...document.querySelectorAll('[data-slot="empty"]')].map((e) => ({
      title: e.querySelector('[data-slot="empty-title"]')?.textContent ?? '',
      retry: [...e.querySelectorAll('button')].some((b) =>
        /try again|retry/i.test(b.textContent ?? ''),
      ),
    })),
    h1: [...document.querySelectorAll('h1')].map((h) => h.textContent),
  }));
}

for (const route of APP_ROUTES) {
  if (ONLY && !ONLY.includes(route.name)) continue;
  if (route.signedOut) continue;
  for (const mode of ['loading', 'error'] as const) {
    for (const width of [375, 1280] as const) {
      test(`${route.name} ${mode} light ${width}`, async ({ page }) => {
        await page.setViewportSize({ width, height: HEIGHT[width] });
        await openRoute(page, route, {
          mode,
          theme: 'light',
          settle: mode === 'loading' ? 1_500 : 10_000,
        });
        const shot = join(AUDIT_DIR, 'shots', mode, 'light', String(width), `${route.name}.png`);
        mkdirSync(dirname(shot), { recursive: true });
        await page.screenshot({ path: shot, animations: 'disabled' });
        const record = { route: route.name, mode, width, state: await stateRecord(page) };
        const out = join(AUDIT_DIR, 'states', `${route.name}.${mode}.${width}.json`);
        mkdirSync(dirname(out), { recursive: true });
        writeFileSync(out, JSON.stringify(record, null, 2));
      });
    }
  }
}
