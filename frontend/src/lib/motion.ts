import { AnimatePresence, domAnimation, LazyMotion, MotionConfig } from 'motion/react';
import { createElement, type ReactNode } from 'react';

/**
 * Shared motion for things that appear, disappear or change size (a panel, a row, an entry).
 * Entrances that only fade content in stay CSS (`FadeIn`, `motion-safe:animate-fade-in`).
 */

/** The spring panels and rows move with: quick, with a short settle (the assistant panel's). */
export const spring = { type: 'spring', stiffness: 420, damping: 34, mass: 0.8 } as const;

/** An entry rising into place, and fading out quickly when it goes. */
export const rise = {
  initial: { opacity: 0, y: 10 },
  animate: { opacity: 1, y: 0 },
  exit: { opacity: 0, y: -4, transition: { duration: 0.12 } },
  transition: spring,
} as const;

/** A row opening to its height and closing back to nothing, so the rows below it slide. */
export const unfold = {
  initial: { opacity: 0, height: 0 },
  animate: { opacity: 1, height: 'auto' },
  exit: { opacity: 0, height: 0, transition: { duration: 0.14 } },
  transition: { ...spring, stiffness: 520, damping: 42 },
} as const;

/**
 * Where `m.*` children appear and disappear: the small DOM feature set (no layout animation),
 * movement only for readers who did not ask for less, and nothing animated on first render, so
 * what is there when the page opens is simply there.
 */
export function MotionPresence({ children }: { children: ReactNode }) {
  return createElement(
    LazyMotion,
    { features: domAnimation },
    createElement(
      MotionConfig,
      { reducedMotion: 'user' },
      createElement(AnimatePresence, { initial: false }, children),
    ),
  );
}
