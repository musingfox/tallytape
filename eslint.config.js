import tseslint from "typescript-eslint";
import reactHooks from "eslint-plugin-react-hooks";
import reactRefresh from "eslint-plugin-react-refresh";
import globals from "globals";
import eslintConfigPrettier from "eslint-config-prettier";

export default tseslint.config(
  {
    ignores: [
      "dist",
      "target",
      "src-tauri/target",
      "src-tauri/gen",
      "node_modules",
      "src/routeTree.gen.ts",
      "tests/e2e/.features-gen/**",
    ],
  },
  ...tseslint.configs.recommended,
  {
    files: ["**/*.{ts,tsx}"],
    languageOptions: {
      ecmaVersion: 2022,
      globals: globals.browser,
    },
    plugins: {
      "react-hooks": reactHooks,
      "react-refresh": reactRefresh,
    },
    rules: {
      ...reactHooks.configs.recommended.rules,
      "react-refresh/only-export-components": ["warn", { allowConstantExport: true }],
    },
  },
  eslintConfigPrettier,
  {
    files: ["src/routes/**/*.{ts,tsx}"],
    rules: {
      "react-refresh/only-export-components": "off",
    },
  },
  {
    files: ["tests/e2e/**/*.ts"],
    languageOptions: {
      globals: { ...globals.node, ...globals.browser },
    },
    rules: {
      // playwright-bdd's fixture API uses `use(...)` to yield the fixture
      // value — `react-hooks/rules-of-hooks` cannot tell that apart from the
      // React `use` hook, so silence the false positives in this directory.
      "react-hooks/rules-of-hooks": "off",
    },
  },
  {
    files: ["src/**/*.{ts,tsx}"],
    ignores: ["src/ipc/**", "src/**/__tests__/**", "src/**/*.test.{ts,tsx}"],
    rules: {
      "no-restricted-imports": [
        "error",
        {
          paths: [
            {
              name: "@tauri-apps/api/core",
              importNames: ["invoke"],
              message: "Import wrappers from src/ipc instead of calling invoke directly.",
            },
          ],
        },
      ],
    },
  },
);
