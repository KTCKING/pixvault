import { getTauriVersion, getVersion } from "@tauri-apps/api/app";

export interface AppInfo {
  /** 应用版本，和 `tauri.conf.json` 里的 `version` 一致。 */
  version: string;
  /** Tauri 运行时版本，出问题时用来判断内核差异。 */
  tauriVersion: string;
  /** 开发版还是发行版（`vite dev` 下是开发版）。 */
  mode: "dev" | "release";
  /** 前端产物的构建时间（`YYYY-MM-DD HH:mm`，本地时区）。 */
  buildTime: string;
}

/**
 * 构建时间已经是「年月日 时分」了（见 `vite.config.ts` 里的 `__BUILD_TIME__`），
 * 这里只兜一下带 `T` 的 ISO 串，避免以后改了注入格式就显示成一整串。
 */
function fmtBuildTime(stamp: string): string {
  const m = stamp.match(/^(\d{4}-\d{2}-\d{2})[T ](\d{2}:\d{2})/);
  return m ? `${m[1]} ${m[2]}` : stamp;
}

/**
 * 读取运行时版本信息。
 *
 * 优先问 Tauri 内核（`getVersion` 读的就是打包时写进去的版本号，改
 * `package.json` 不会影响它）。不在 Tauri 里跑（比如直接用浏览器打开
 * `localhost:1420`）时会抛错，这时退回到构建时注入的 `__APP_VERSION__`，
 * 让页面照样能渲染，只是标注成「浏览器预览」。
 */
export async function loadAppInfo(): Promise<AppInfo> {
  const mode: AppInfo["mode"] = import.meta.env.DEV ? "dev" : "release";
  const buildTime = fmtBuildTime(__BUILD_TIME__);

  try {
    const [version, tauriVersion] = await Promise.all([getVersion(), getTauriVersion()]);
    return { version, tauriVersion, mode, buildTime };
  } catch {
    return {
      version: __APP_VERSION__,
      tauriVersion: "—",
      mode,
      buildTime,
    };
  }
}
