/// <reference types="vite/client" />

/** 构建时注入：`package.json` 里的版本号（Tauri 不可用时的兜底）。 */
declare const __APP_VERSION__: string;

/** 构建时注入：前端产物的构建时间，ISO 8601。 */
declare const __BUILD_TIME__: string;
