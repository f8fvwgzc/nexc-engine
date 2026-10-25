/**
 * motion's animation code, apart from the `m` components that use it. A `LazyMotion` loads this
 * module when it mounts (the assistant's, on every page of the app), after the shell has drawn,
 * so neither the shell's chunk nor a page's carries it (see motion-presets).
 */
export { domAnimation, domMax } from 'motion/react';
