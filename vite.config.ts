import { defineConfig } from "vite";
import { resolve } from "path";
import vue from "@vitejs/plugin-vue";
import { VueMcp } from "vite-plugin-vue-mcp";
import AutoImport from "unplugin-auto-import/vite";
import { NaiveUiResolver } from "unplugin-vue-components/resolvers";
import Components from "unplugin-vue-components/vite";
import Icons from "unplugin-icons/vite";
import IconsResolver from "unplugin-icons/resolver";

const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [
    vue(),
    VueMcp(),
    AutoImport({
      imports: [
        "vue",
        "vue-router",
        "@vueuse/core",
        {
          "naive-ui": ["useDialog", "useMessage", "useNotification", "useLoadingBar"],
        },
      ],
      eslintrc: {
        enabled: true,
        filepath: "./auto-eslint.mjs",
      },
    }),
    Components({
      resolvers: [NaiveUiResolver(), IconsResolver({ prefix: "icon" })],
    }),
    Icons({
      compiler: "vue3",
      autoInstall: false,
    }),
  ],
  resolve: {
    alias: {
      "@": resolve(__dirname, "src"),
    },
  },
  // Prevent Vite from obscuring Rust/Tauri CLI compiler logs
  clearScreen: false,
  server: {
    port: 15688,
    // Tauri expects a fixed port matching devUrl in tauri.conf.json
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 15689,
        }
      : undefined,
    watch: {
      // Tell Vite to ignore watching src-tauri
      ignored: ["**/src-tauri/**"],
    },
  },
  // Expose both VITE_ and TAURI_ENV_ prefixed variables to the frontend
  envPrefix: ["VITE_", "TAURI_ENV_*"],
}));
