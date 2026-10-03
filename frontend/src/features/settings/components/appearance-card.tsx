import { GlassCard } from '@/components/custom-ui/glass-card';
import { isTheme, THEME_OPTIONS } from '@/components/layout/theme-options';
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group';
import { useTheme } from '@/hooks/use-theme';

export function AppearanceCard() {
  const { theme, setTheme } = useTheme();
  return (
    <GlassCard className="flex flex-col gap-4 p-5 sm:flex-row sm:items-center sm:justify-between sm:p-6">
      <div className="space-y-1">
        <h2 className="font-medium">Appearance</h2>
        <p className="text-sm text-muted-foreground">Dark-first, or follow your system setting.</p>
      </div>
      <ToggleGroup
        type="single"
        variant="outline"
        value={theme}
        onValueChange={(v) => isTheme(v) && setTheme(v)}
        aria-label="Theme"
      >
        {THEME_OPTIONS.map(({ value, label, icon: Icon }) => (
          <ToggleGroupItem
            key={value}
            value={value}
            aria-label={`${label} theme`}
            className="gap-1.5 px-3"
          >
            <Icon />
            {label}
          </ToggleGroupItem>
        ))}
      </ToggleGroup>
    </GlassCard>
  );
}
