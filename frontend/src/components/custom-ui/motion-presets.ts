import { useState, type CSSProperties } from 'react';

import { retryImport } from '@/lib/retry-import';

/*
 * The shared motion of the app's moving parts (motion/react). Plain values and one hook, so
 * importing them pulls in none of motion's code: use them with `m.*` components inside
 * `<LazyMotion features={…}>` and `<MotionConfig reducedMotion="user">`. Entrances that only fade
 * or rise use the CSS ones in motion.tsx instead, and staggerStyle below.
 */

/**
 * The delay of the `index`th item of a list whose items fade in one after another, as Stagger's
 * do, for items that carry `motion-safe:animate-fade-up` themselves: `li`s, which Stagger's
 * wrappers would break. CSS, like Stagger.
 */
export function staggerStyle(index: number, stepMs = 45, maxDelayMs = 400): CSSProperties {
  return { animationDelay: `${Math.min(index * stepMs, maxDelayMs)}ms` };
}

/** The spring of panels and rows: quick, and settled without a wobble. */
export const spring = { type: 'spring', stiffness: 420, damping: 34, mass: 0.8 } as const;

/** Something that rises into a list as it arrives, and lifts away briefly when it goes. */
export const rise = {
  initial: { opacity: 0, y: 10 },
  animate: { opacity: 1, y: 0 },
  exit: { opacity: 0, y: -4, transition: { duration: 0.12 } },
  transition: spring,
} as const;

/** Appear, disappear and animate values: what panels, notches and new entries need. */
export const loadAnimation = retryImport(() =>
  import('./motion-features').then((m) => m.domAnimation),
);

/** All of the above plus `layout` and drag: rows that move when their neighbours come and go. */
export const loadLayoutAnimation = retryImport(() =>
  import('./motion-features').then((m) => m.domMax),
);

/**
 * The features to give `<LazyMotion>`, and whether they could not be loaded. Without them an
 * `m.*` part keeps its `initial` values for good (a panel that opens from width 0 never shows),
 * so while `still` is true the caller shows each part where its animation would end: it passes
 * `initial={false}` to each, and keys the subtree on `still` so the parts already there mount
 * anew that way.
 */
export function useMotionFeatures<T>(load: () => Promise<T>): {
  features: () => Promise<T>;
  still: boolean;
} {
  const [still, setStill] = useState(false);
  // LazyMotion calls it once, when it mounts.
  const [features] = useState(
    () => () =>
      load().catch(() => {
        setStill(true);
        // Never settles: motion stays unloaded, and no rejection is left unhandled.
        return new Promise<T>(() => undefined);
      }),
  );
  return { features, still };
}
