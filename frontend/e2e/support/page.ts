import AxeBuilder from '@axe-core/playwright';
import { expect, type Page } from '@playwright/test';

import { mockApi, type DataMode, type MockLog } from './mock-api';
import type { AppRoute } from './routes';

/** The moment every fixture's timestamps are relative to. */
export const NOW = new Date('2026-10-14T12:00:00Z');

export interface OpenOptions {
  mode: DataMode;
  theme: 'light' | 'dark';
  /** How long to wait for loading placeholders to go (default 10 s). */
  settle?: number;
}

export interface Opened extends MockLog {
  /** Loading placeholders still shown once the page had time to settle (0 when it did). */
  stuckLoading: number;
}

/**
 * Opens a route against the mocked API at a fixed time and waits until it has settled: no
 * skeleton or busy region left, fonts loaded, and two quiet frames.
 */
export async function openRoute(
  page: Page,
  route: AppRoute,
  options: OpenOptions,
): Promise<Opened> {
  await page.emulateMedia({ colorScheme: options.theme, reducedMotion: 'reduce' });
  await page.clock.install({ time: NOW });
  const log = await mockApi(page, {
    mode: options.mode,
    persona: route.persona,
    signedOut: route.signedOut,
  });
  await page.goto(route.path);
  const stuckLoading = await waitForSettled(page, options.settle);
  return Object.assign(log, { stuckLoading });
}

const LOADING = '[aria-busy="true"], [data-slot="skeleton"], [aria-label="Loading nexc-engine"]';

/** Waits for the page to settle; answers how many loading placeholders never went away. */
export async function waitForSettled(page: Page, timeout = 10_000): Promise<number> {
  await expect(page.locator('#root')).not.toBeEmpty();
  const loading = () =>
    page.evaluate((selector) => document.querySelectorAll(selector).length, LOADING);
  try {
    await expect.poll(loading, { timeout }).toBe(0);
  } catch {
    // Reported by the caller: a placeholder that never resolves is a finding, not a crash.
  }
  await page.evaluate(async () => {
    await document.fonts.ready;
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  });
  return loading();
}

/**
 * The app shell is one viewport tall and scrolls inside `<main>`. To see a whole page, the
 * viewport is made as tall as the page's content (capped) before a screenshot.
 */
export async function fitViewportToContent(
  page: Page,
  width: number,
  minHeight: number,
  cap = 4000,
) {
  const contentHeight = await page.evaluate(() => {
    const scroller = document.querySelector('#main > .overflow-y-auto, #main .overflow-y-auto');
    const doc = document.scrollingElement?.scrollHeight ?? 0;
    if (!(scroller instanceof HTMLElement)) return doc;
    const header = scroller.getBoundingClientRect().top;
    return Math.max(doc, header + scroller.scrollHeight);
  });
  const height = Math.min(cap, Math.max(minHeight, Math.ceil(contentHeight)));
  await page.setViewportSize({ width, height });
  await waitForSettled(page, 2_000);
}

export interface Overlap {
  a: string;
  b: string;
  area: number;
}

export interface LayoutReport {
  /** `document.scrollingElement.scrollWidth - innerWidth` (0 when the page fits). */
  docOverflow: number;
  /** Horizontal overflow of the page's own scroller (`<main>`'s content). */
  mainOverflow: number;
  overlaps: Overlap[];
}

/**
 * The layout check: no horizontal scroll, and no two visible interactive elements whose boxes
 * intersect (nested ones and fixed overlays such as the assistant button excepted).
 */
export async function checkLayout(page: Page): Promise<LayoutReport> {
  return page.evaluate(() => {
    const SELECTOR = [
      'a[href]',
      'button',
      'input:not([type="hidden"])',
      'select',
      'textarea',
      '[role="button"]',
      '[role="tab"]',
      '[role="checkbox"]',
      '[role="switch"]',
      '[role="combobox"]',
      '[role="menuitem"]',
      '[tabindex]:not([tabindex="-1"])',
    ].join(',');

    const describe = (el: Element) => {
      const label =
        el.getAttribute('aria-label') ??
        (el as HTMLElement).innerText?.trim().split('\n')[0]?.slice(0, 40) ??
        '';
      const id = el.id ? `#${el.id}` : '';
      return `${el.tagName.toLowerCase()}${id}[${label}]`;
    };

    const isFixed = (el: Element) => {
      for (let node: Element | null = el; node; node = node.parentElement) {
        const position = getComputedStyle(node).position;
        if (position === 'fixed' || position === 'sticky') return true;
      }
      return false;
    };

    /** The part of the element not clipped away by a scrolling or clipping ancestor. */
    const visibleRect = (el: Element) => {
      const r = el.getBoundingClientRect();
      let left = r.left;
      let top = r.top;
      let right = r.right;
      let bottom = r.bottom;
      for (let node = el.parentElement; node; node = node.parentElement) {
        const style = getComputedStyle(node);
        if (style.overflowX !== 'visible' || style.overflowY !== 'visible') {
          const c = node.getBoundingClientRect();
          left = Math.max(left, c.left);
          top = Math.max(top, c.top);
          right = Math.min(right, c.right);
          bottom = Math.min(bottom, c.bottom);
        }
      }
      return { left, top, right, bottom };
    };

    const visible = (el: Element) => {
      const style = getComputedStyle(el);
      if (style.visibility === 'hidden' || style.display === 'none' || Number(style.opacity) === 0)
        return false;
      // Visually hidden until focused (`sr-only`, the skip link).
      if (style.clipPath.startsWith('inset(50%') || style.clip === 'rect(0px, 0px, 0px, 0px)')
        return false;
      if (el.closest('[aria-hidden="true"], [inert], [data-sonner-toaster]')) return false;
      const r = el.getBoundingClientRect();
      return r.width > 1 && r.height > 1;
    };

    const elements = [...document.querySelectorAll(SELECTOR)].filter(
      (el) => visible(el) && !isFixed(el),
    );
    const boxes = elements
      .map((el) => ({ el, r: visibleRect(el) }))
      .filter(({ r }) => r.right - r.left > 1 && r.bottom - r.top > 1);

    const overlaps: { a: string; b: string; area: number }[] = [];
    for (let i = 0; i < boxes.length; i++) {
      for (let j = i + 1; j < boxes.length; j++) {
        const a = boxes[i]!;
        const b = boxes[j]!;
        if (a.el.contains(b.el) || b.el.contains(a.el)) continue;
        const w = Math.min(a.r.right, b.r.right) - Math.max(a.r.left, b.r.left);
        const h = Math.min(a.r.bottom, b.r.bottom) - Math.max(a.r.top, b.r.top);
        // A shared border pixel (rows, segmented controls) is not an overlap.
        if (w > 2 && h > 2)
          overlaps.push({ a: describe(a.el), b: describe(b.el), area: Math.round(w * h) });
      }
    }

    const scroller = document.querySelector('#main > .overflow-y-auto, #main .overflow-y-auto');
    const doc = document.scrollingElement;
    return {
      docOverflow: Math.max(0, (doc?.scrollWidth ?? 0) - window.innerWidth),
      mainOverflow:
        scroller instanceof HTMLElement
          ? Math.max(0, scroller.scrollWidth - scroller.clientWidth)
          : 0,
      overlaps,
    };
  });
}

export interface AxeFinding {
  id: string;
  impact: string;
  help: string;
  nodes: string[];
}

/** WCAG 2.2 A/AA rules plus best practices, on the page as it stands. */
export async function runAxe(page: Page): Promise<AxeFinding[]> {
  const results = await new AxeBuilder({ page })
    .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa', 'best-practice'])
    .analyze();
  return results.violations.map((v) => ({
    id: v.id,
    impact: v.impact ?? 'unknown',
    help: v.help,
    nodes: v.nodes.slice(0, 6).map((n) => {
      const why = n.any[0]?.message ?? n.all[0]?.message ?? '';
      return `${n.target.join(' ')}${why ? ` (${why.slice(0, 160)})` : ''}`;
    }),
  }));
}

export const isSeriousOrCritical = (finding: AxeFinding) =>
  finding.impact === 'serious' || finding.impact === 'critical';
