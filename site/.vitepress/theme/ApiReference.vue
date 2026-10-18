<script setup lang="ts">
import { useData, withBase } from 'vitepress';
import { computed, defineAsyncComponent } from 'vue';

import '@scalar/api-reference/style.css';

// Scalar touches the DOM when it loads, so it is imported in the browser only.
const Scalar = defineAsyncComponent(() =>
  import('@scalar/api-reference').then((m) => m.ApiReference),
);
const { isDark } = useData();

// The OpenAPI document is the copy under site/public, refreshed by `make openapi`.
const configuration = computed(() => ({
  url: withBase('/openapi.json'),
  forceDarkModeState: isDark.value ? ('dark' as const) : ('light' as const),
  hideDarkModeToggle: true,
  theme: 'default' as const,
}));
</script>

<template>
  <ClientOnly>
    <div class="api-reference">
      <Scalar :key="String(isDark)" :configuration="configuration" />
    </div>
  </ClientOnly>
</template>
