/**
 * motion's animation code, apart from the `m` components that use it: `LazyMotion` loads this
 * module the first time something animates, so no page chunk carries it (see motion-presets).
 */
export { domAnimation, domMax } from 'motion/react';
