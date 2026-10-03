// Runs synchronously before first paint to avoid a theme flash. Mirrors ThemeProvider.
(function () {
  var theme = 'system';
  try {
    var stored = window.localStorage.getItem('nexc-theme');
    if (stored === 'light' || stored === 'dark' || stored === 'system') theme = stored;
  } catch (_) {
    /* storage unavailable (private mode, blocked cookies) — fall back to system */
  }
  var dark =
    theme === 'dark' ||
    (theme === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches);
  var root = document.documentElement;
  root.classList.toggle('dark', dark);
  root.style.colorScheme = dark ? 'dark' : 'light';
})();
