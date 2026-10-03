import { ArrowRightIcon } from 'lucide-react';
import { Link } from 'react-router-dom';

import { AnimatedButton } from '@/components/custom-ui/animated-button';
import { GlassCard } from '@/components/custom-ui/glass-card';
import { GradientText } from '@/components/custom-ui/gradient-text';
import { FadeIn } from '@/components/custom-ui/motion';
import { BrandMark } from '@/components/layout/brand-mark';
import { ThemeToggle } from '@/components/layout/theme-toggle';
import { Seo } from '@/components/seo/seo';
import { Button } from '@/components/ui/button';
import { FeatureGrid } from '@/features/landing/components/feature-grid';
import { HeroGraph } from '@/features/landing/components/hero-graph';

const GITHUB_URL = 'https://github.com/f8fvwgzc/nexc-engine';

function GitHubIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden className="size-4 fill-current">
      <path d="M12 .5a11.5 11.5 0 0 0-3.64 22.41c.58.1.79-.25.79-.56v-2c-3.2.7-3.88-1.37-3.88-1.37-.52-1.33-1.28-1.69-1.28-1.69-1.05-.71.08-.7.08-.7 1.16.08 1.77 1.19 1.77 1.19 1.03 1.77 2.71 1.26 3.37.96.1-.75.4-1.26.73-1.55-2.55-.29-5.24-1.28-5.24-5.69 0-1.26.45-2.29 1.19-3.09-.12-.29-.52-1.46.11-3.05 0 0 .97-.31 3.17 1.18a11 11 0 0 1 5.77 0c2.2-1.49 3.17-1.18 3.17-1.18.63 1.59.23 2.76.11 3.05.74.8 1.19 1.83 1.19 3.09 0 4.42-2.7 5.39-5.26 5.68.41.36.78 1.06.78 2.14v3.17c0 .31.21.67.8.56A11.5 11.5 0 0 0 12 .5Z" />
    </svg>
  );
}

export default function LandingPage() {
  return (
    <div className="relative isolate min-h-svh overflow-x-hidden bg-background">
      <Seo description="nexc-engine: plan a goal as a graph of LLM tasks, refine it with streaming plans, and run it as a live DAG. Open source." />
      <div
        aria-hidden
        className="pointer-events-none absolute inset-0 -z-10 bg-[radial-gradient(70rem_40rem_at_50%_-10%,color-mix(in_oklch,var(--brand)_20%,transparent),transparent),radial-gradient(40rem_30rem_at_100%_40%,color-mix(in_oklch,var(--brand-2)_12%,transparent),transparent)]"
      />
      <header className="mx-auto flex max-w-6xl items-center justify-between gap-2 px-4 py-4 sm:px-6">
        <Link to="/" className="flex items-center gap-2 font-semibold tracking-tight">
          <BrandMark className="size-7" />
          nexc-engine
        </Link>
        <nav className="flex items-center gap-1" aria-label="Main">
          <Button variant="ghost" size="icon" asChild>
            <a
              href={GITHUB_URL}
              target="_blank"
              rel="noopener noreferrer"
              aria-label="nexc-engine on GitHub"
            >
              <GitHubIcon />
            </a>
          </Button>
          <ThemeToggle />
          <Button variant="ghost" asChild className="hidden sm:inline-flex">
            <Link to="/login">Log in</Link>
          </Button>
          <Button asChild>
            <Link to="/register">Get started</Link>
          </Button>
        </nav>
      </header>

      <main id="main">
        <section className="mx-auto grid max-w-6xl items-center gap-10 px-4 pt-10 pb-16 sm:px-6 lg:grid-cols-[1.05fr_1fr] lg:pt-20">
          <FadeIn className="space-y-6">
            <p className="inline-flex items-center gap-2 rounded-full border bg-background/60 px-3 py-1 text-xs text-muted-foreground backdrop-blur">
              <span className="size-1.5 rounded-full bg-status-succeeded" aria-hidden />
              Open source · self-hostable · works offline in demo mode
            </p>
            <h1 className="text-4xl font-semibold tracking-tight text-balance sm:text-5xl lg:text-6xl">
              Plan it as a graph. <GradientText>Run it as a DAG.</GradientText>
            </h1>
            <p className="max-w-xl text-lg text-pretty text-muted-foreground">
              nexc-engine turns a big goal — a research report, an API, a launch plan — into
              connected tasks, lets an LLM refine the plan live, and executes everything in
              dependency order with streaming output and real artifacts.
            </p>
            <div className="flex flex-wrap gap-3">
              <AnimatedButton glow size="lg" asChild>
                <Link to="/register">
                  Start for free
                  <ArrowRightIcon />
                </Link>
              </AnimatedButton>
              <Button variant="outline" size="lg" asChild>
                <a href={GITHUB_URL} target="_blank" rel="noopener noreferrer">
                  <GitHubIcon />
                  Star on GitHub
                </a>
              </Button>
            </div>
          </FadeIn>
          <FadeIn delayMs={120} variant="scale">
            <GlassCard className="graph-canvas overflow-hidden p-3 sm:p-5">
              <HeroGraph />
            </GlassCard>
          </FadeIn>
        </section>

        <section
          aria-labelledby="features"
          className="mx-auto max-w-6xl space-y-8 px-4 pb-20 sm:px-6"
        >
          <h2
            id="features"
            className="text-center text-2xl font-semibold tracking-tight sm:text-3xl"
          >
            Everything between “idea” and “report.docx”
          </h2>
          <FeatureGrid />
        </section>

        <section className="mx-auto max-w-6xl px-4 pb-24 sm:px-6">
          <GlassCard className="flex flex-col items-start gap-4 p-6 sm:flex-row sm:items-center sm:justify-between sm:p-8">
            <div className="space-y-1">
              <h2 className="text-xl font-semibold tracking-tight">Try it in two minutes</h2>
              <p className="text-sm text-muted-foreground">
                Pick a template, press Run. No API key needed in demo mode.
              </p>
            </div>
            <div className="flex gap-2">
              <Button variant="outline" asChild>
                <Link to="/login">Log in</Link>
              </Button>
              <AnimatedButton glow asChild>
                <Link to="/register">Create an account</Link>
              </AnimatedButton>
            </div>
          </GlassCard>
        </section>
      </main>

      <footer className="border-t">
        <div className="mx-auto flex max-w-6xl flex-col gap-2 px-4 py-6 text-sm text-muted-foreground sm:flex-row sm:items-center sm:justify-between sm:px-6">
          <span>© nexc-engine contributors</span>
          <a
            href={GITHUB_URL}
            target="_blank"
            rel="noopener noreferrer"
            className="underline-offset-4 hover:text-foreground hover:underline"
          >
            github.com/f8fvwgzc/nexc-engine
          </a>
        </div>
      </footer>
    </div>
  );
}
