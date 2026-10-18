<script setup lang="ts">
import { useData } from 'vitepress';
import { onMounted, ref, useId, watch } from 'vue';

const props = defineProps<{ code: string }>();
const { isDark } = useData();
const svg = ref('');
const id = useId();
let renders = 0;

async function render() {
  const mermaid = (await import('mermaid')).default;
  mermaid.initialize({
    startOnLoad: false,
    theme: isDark.value ? 'dark' : 'neutral',
    fontFamily: 'inherit',
  });
  // Every render needs a fresh element id; mermaid cleans the previous one up.
  const out = await mermaid.render(`mermaid-${id}-${renders++}`, decodeURIComponent(props.code));
  svg.value = out.svg;
}

onMounted(render);
watch(isDark, render);
</script>

<template>
  <div class="mermaid" v-html="svg" />
</template>
