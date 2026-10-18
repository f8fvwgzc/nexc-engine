import type { Theme } from 'vitepress';
import DefaultTheme from 'vitepress/theme';

import ApiReference from './ApiReference.vue';
import Mermaid from './Mermaid.vue';
import './custom.css';

export default {
  extends: DefaultTheme,
  enhanceApp({ app }) {
    app.component('Mermaid', Mermaid);
    app.component('ApiReference', ApiReference);
  },
} satisfies Theme;
