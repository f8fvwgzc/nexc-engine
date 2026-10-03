import { BrandMark } from './brand-mark';

/** Full-screen placeholder while the session is restored from the refresh cookie. */
export function BootSplash() {
  return (
    <div
      role="status"
      aria-label="Loading nexc-engine"
      className="flex min-h-svh items-center justify-center bg-background"
    >
      <BrandMark className="size-12 motion-safe:animate-pulse" />
    </div>
  );
}
