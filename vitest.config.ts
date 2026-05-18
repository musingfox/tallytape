import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: ['./src/test/setup.ts'],
    // Playwright + Rust BDD suites have their own runners — keep vitest focused on
    // the unit tests under src/.
    include: ['src/**/*.{test,spec}.{ts,tsx}'],
    exclude: ['tests/**', 'node_modules/**', 'dist/**'],
  },
});
