import { defineConfig } from "vite";
import solid from "vite-plugin-solid";
import { readFileSync } from "fs";
import { resolve } from "path";

const host = process.env.TAURI_DEV_HOST;

// 设置面板要显示「版本 + 构建时间」，这两样只能在这里注入：
// 版本号跟 tauri.conf.json 保持一致（运行时优先问 Tauri 内核，这里只做兜底），
// 构建时间则是纯粹的前端概念，运行时无从得知。
const pkg = JSON.parse(readFileSync(resolve(__dirname, "package.json"), "utf-8")) as {
  version: string;
};

/**
 * 构建时间按**本地时区**写成 `YYYY-MM-DD HH:mm`。
 *
 * 不用 `toISOString()`：那是 UTC，界面上会显示成比实际早 8 小时的时间，
 * 看着像打包时间错乱。构建机器和用户同一时区，直接取本地时间最省事。
 */
const pad = (n: number) => String(n).padStart(2, "0");
const now = new Date();
const buildTime = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())} ${pad(now.getHours())}:${pad(now.getMinutes())}`;

export default defineConfig(async () => ({
  plugins: [solid()],
  define: {
    __APP_VERSION__: JSON.stringify(pkg.version),
    __BUILD_TIME__: JSON.stringify(buildTime),
  },
  resolve: {
    alias: {
      "@": resolve(__dirname, "./src"),
    },
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
}));