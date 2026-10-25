/*
 * The shared motion of the app's moving parts (motion/react). Plain values, so importing them
 * pulls in nothing: use them with `m.*` components inside `<LazyMotion features={…}>` and
 * `<MotionConfig reducedMotion="user">`. Entrances that only fade or rise use the CSS ones in
 * motion.tsx instead.
 */

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
export const loadAnimation = () => import('./motion-features').then((m) => m.domAnimation);

/** All of the above plus `layout` and drag: rows that move when their neighbours come and go. */
export const loadLayoutAnimation = () => import('./motion-features').then((m) => m.domMax);
