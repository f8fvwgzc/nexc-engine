const compact = new Intl.NumberFormat('en', { notation: 'compact', maximumFractionDigits: 1 });
const integer = new Intl.NumberFormat('en');
const usd = new Intl.NumberFormat('en', {
  style: 'currency',
  currency: 'USD',
  minimumFractionDigits: 2,
  maximumFractionDigits: 4,
});
const relative = new Intl.RelativeTimeFormat('en', { numeric: 'auto' });
const dateTime = new Intl.DateTimeFormat('en', { dateStyle: 'medium', timeStyle: 'short' });

export function formatTokens(n: number): string {
  return n < 10_000 ? integer.format(n) : compact.format(n);
}

export function formatInteger(n: number): string {
  return integer.format(n);
}

export function formatCost(usdAmount: number): string {
  return usd.format(usdAmount);
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ['KB', 'MB', 'GB'];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(value < 10 ? 1 : 0)} ${units[unit]}`;
}

export function formatDuration(ms: number): string {
  if (ms < 1000) return `${Math.max(0, Math.round(ms))} ms`;
  const s = ms / 1000;
  if (s < 60) return `${s.toFixed(s < 10 ? 1 : 0)} s`;
  const m = Math.floor(s / 60);
  const rest = Math.round(s % 60);
  if (m < 60) return `${m}m ${rest}s`;
  return `${Math.floor(m / 60)}h ${m % 60}m`;
}

export function durationBetween(start: string | null, end: string | null): number | null {
  if (!start) return null;
  const endMs = end ? Date.parse(end) : Date.now();
  return endMs - Date.parse(start);
}

export function formatRelative(iso: string, now = Date.now()): string {
  const diffSec = Math.round((Date.parse(iso) - now) / 1000);
  const abs = Math.abs(diffSec);
  if (abs < 45) return relative.format(diffSec, 'second');
  if (abs < 2700) return relative.format(Math.round(diffSec / 60), 'minute');
  if (abs < 79_200) return relative.format(Math.round(diffSec / 3600), 'hour');
  if (abs < 2_592_000) return relative.format(Math.round(diffSec / 86_400), 'day');
  return dateTime.format(new Date(iso));
}

export function formatDateTime(iso: string): string {
  return dateTime.format(new Date(iso));
}
