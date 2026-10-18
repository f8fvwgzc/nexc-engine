import { createContext, use, type ReactNode } from 'react';

const InsideMain = createContext(false);

/** Marks what renders inside a shell's `<main>`, so a page does not open a second one. */
export function MainLandmarkProvider({ children }: { children: ReactNode }) {
  return <InsideMain value>{children}</InsideMain>;
}

/**
 * A page's main landmark: a `<main>` of its own where no shell gave it one (a 404 or an error
 * outside the app), a plain `<div>` inside a shell's.
 */
export function MainLandmark({ className, children }: { className?: string; children: ReactNode }) {
  const inside = use(InsideMain);
  return inside ? (
    <div className={className}>{children}</div>
  ) : (
    <main className={className}>{children}</main>
  );
}
