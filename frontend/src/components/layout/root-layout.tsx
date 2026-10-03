import { Outlet, ScrollRestoration } from 'react-router-dom';

import { NavigationProgress } from './navigation-progress';

/**
 * Top-level route element. Head tags come from exactly one <Seo> per page: under React 19,
 * react-helmet-async renders native hoisted tags without de-duplication.
 */
export function RootLayout() {
  return (
    <>
      <NavigationProgress />
      <Outlet />
      <ScrollRestoration />
    </>
  );
}
