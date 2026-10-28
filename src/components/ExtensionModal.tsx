import { Show, createSignal, onMount, onCleanup } from "solid-js";
import { api, type BridgeStatus } from "@/lib/tauri-api";
import { useUIStore } from "@/stores/ui";
import { open } from "@tauri-apps/plugin-dialog";
import { IconClose, IconPuzzle } from "@/components/icons";

interface Props {
  onClose: () => void;
}

const fmtBytes = (n: number): string => {
  if (!n) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  let i = 0;
  let v = n;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  return `${v >= 100 || i === 0 ? Math.round(v) : v.toFixed(1)} ${units[i]}`;
};

/**
 * 浏览器扩展设置。
 *
 * 这个面板要回答三个问题：**通不通**（端口 + 令牌）、**装到哪**（导出扩展）、
 * **刚才存了什么**（最近记录）。所以三块从上到下依次排 —— 连不上时用户第一眼
 * 就能看到原因，而不是在几个标签页里找。
 */
export function ExtensionModal(props: Props) {
  const ui = useUIStore();
  const [status, setStatus] = createSignal<BridgeStatus | null>(null);
  const [portDraft, setPortDraft] = createSignal("");
  const [showToken, setShowToken] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  const [exportedAt, setExportedAt] = createSignal("");

  const refresh = async () => {
    try {
      const s = await api.getBridgeStatus();
      setStatus(s);
      if (!portDraft()) setPortDraft(String(s.port));
    } catch (e) {
      ui.showToast(`读取扩展状态失败：${String(e)}`, "error");
    }
  };

  onMount(() => {
    void refresh();
    // 监听状态是异步起来的：轮询比「用户手动点刷新」友好，开销也只是一个本地调用
    const timer = setInterval(() => void refresh(), 3000);
    onCleanup(() => clearInterval(timer));
  });

  const s = {
    primary: "var(--text-primary)",
    secondary: "var(--text-secondary)",
    muted: "var(--text-muted)",
    bg: "var(--bg-tertiary)",
    border: "var(--border-color)",
    accent: "var(--accent)",
  };

  const copy = async (text: string, label: string) => {
    try {
      await navigator.clipboard.writeText(text);
      ui.showToast(`${label}已复制`, "success");
    } catch {
      ui.showToast("复制失败，请手动选中复制", "error");
    }
  };

  const toggle = async (on: boolean) => {
    try {
      setStatus(await api.setBridgeEnabled(on));
    } catch (e) {
      ui.showToast(`切换失败：${String(e)}`, "error");
    }
  };

  const applyPort = async () => {
    const port = Number(portDraft());
    if (!Number.isFinite(port) || port < 1024 || port > 65535) {
      ui.showToast("端口需要在 1024 – 65535 之间", "error");
      return;
    }
    setBusy(true);
    try {
      setStatus(await api.setBridgePort(port));
      ui.showToast(`端口已改为 ${port}，扩展设置里也要改一下`, "success");
    } catch (e) {
      ui.showToast(String(e), "error");
    } finally {
      setBusy(false);
    }
  };

  const regen = () => {
    ui.requestConfirm({
      title: "重新生成访问令牌",
      message: "生成新令牌后，已经装好的扩展会连不上，需要去扩展设置里更新一次。图库里的文件不受影响。",
      confirmText: "生成新令牌",
      onConfirm: async () => {
        try {
          const next = await api.regenerateBridgeToken();
          setStatus(next);
          setShowToken(true);
          ui.showToast("已生成新令牌，记得更新扩展设置", "success");
        } catch (e) {
          ui.showToast(`生成失败：${String(e)}`, "error");
        }
      },
    });
  };

  const exportExt = async () => {
    const dest = await open({
      directory: true,
      multiple: false,
      title: "选择把扩展导出到哪个文件夹",
    });
    if (!dest) return;
    setBusy(true);
    try {
      const n = await api.exportExtension(dest as string);
      setExportedAt(`${dest}/pixvault-extension`);
      ui.showToast(`已导出 ${n} 个文件`, "success");
    } catch (e) {
      ui.showToast(`导出失败：${String(e)}`, "error");
    } finally {
      setBusy(false);
    }
  };

  const installSteps = [
    "在浏览器地址栏打开 chrome://extensions（Edge 用 edge://extensions），右上角打开「开发者模式」",
    "点「加载已解压的扩展程序」，选中导出的 pixvault-extension 文件夹",
    "点扩展图标 →「扩展设置」，把下面的端口和令牌填进去，点「测试连接」",
  ];

  return (
    <div class="fixed inset-0 z-50 flex items-center justify-center" style={{ background: "rgba(0,0,0,0.5)" }}>
      <div
        class="glass-modal rounded-xl p-6 w-[560px] max-w-[92vw] max-h-[88vh] overflow-y-auto animate-scale-in"
        style={{ color: s.primary }}
      >
        <div class="flex items-center gap-2.5 mb-4">
          <span style={{ color: s.secondary }}><IconPuzzle size={18} /></span>
          <h2 class="text-lg font-medium flex-1" style={{ color: s.primary }}>浏览器扩展</h2>
          <button class="btn-ghost text-sm px-2 flex" onClick={props.onClose} title="关闭">
            <IconClose size={15} />
          </button>
        </div>

        {/* ── 连接状态 ── */}
        <div
          class="px-4 py-3 rounded-lg mb-4"
          style={{ background: s.bg, border: `1px solid ${s.border}` }}
        >
          <div class="flex items-center gap-2 text-sm">
            <span
              class="w-2 h-2 rounded-full flex-shrink-0"
              style={{
                background: status()?.listening ? "#3ec46d" : status()?.enabled ? "#e0a53a" : "var(--text-muted)",
              }}
            />
            <span style={{ color: s.primary }}>
              {status()?.listening
                ? `本地服务运行中 · 127.0.0.1:${status()!.bound_port}`
                : status()?.enabled
                  ? "本地服务未在监听"
                  : "扩展支持已关闭"}
            </span>
            <div class="flex-1" />
            <label class="flex items-center gap-2 text-xs cursor-pointer" style={{ color: s.secondary }}>
              <input
                type="checkbox"
                checked={status()?.enabled ?? false}
                onChange={(e) => toggle(e.currentTarget.checked)}
              />
              启用
            </label>
          </div>

          <Show when={status()?.last_error}>
            <div class="text-xs mt-2 leading-relaxed" style={{ color: "var(--danger)" }}>
              {status()!.last_error}
            </div>
          </Show>

          <div class="text-xs mt-2 leading-relaxed" style={{ color: s.muted }}>
            只监听本机回环地址，不接受外部连接；请求必须带下面的令牌，网页脚本拿不到它。
            <Show when={status()?.library}>
              <span style={{ color: s.secondary }}>
                {" "}当前库：{status()!.library!.name}
              </span>
            </Show>
          </div>

          <div class="flex items-center gap-3 mt-3 flex-wrap">
            <div class="flex items-center gap-1.5 text-xs" style={{ color: s.secondary }}>
              <span>端口</span>
              <input
                type="number"
                min={1024}
                max={65535}
                class="w-[5.5rem] px-2 py-1 rounded text-xs tabular-nums"
                style={{ background: "var(--bg-secondary)", border: `1px solid ${s.border}`, color: s.primary }}
                value={portDraft()}
                onInput={(e) => setPortDraft(e.currentTarget.value)}
              />
              <button class="btn-secondary text-xs px-2 py-1" disabled={busy()} onClick={applyPort}>
                应用
              </button>
            </div>
            <Show when={status()}>
              <span class="text-2xs tabular-nums" style={{ color: s.muted }}>
                请求 {status()!.requests} · 已存 {status()!.ingested}
              </span>
            </Show>
          </div>
        </div>

        {/* ── 访问令牌 ── */}
        <div class="text-sm mb-2" style={{ color: s.secondary }}>访问令牌</div>
        <div
          class="px-4 py-3 rounded-lg mb-4"
          style={{ background: s.bg, border: `1px solid ${s.border}` }}
        >
          <div class="flex items-center gap-2">
            <code
              class="flex-1 text-xs px-2 py-1.5 rounded truncate tabular-nums tracking-wider"
              style={{
                background: "var(--bg-secondary)",
                border: `1px solid ${s.border}`,
                color: s.primary,
                "font-family": "ui-monospace, SFMono-Regular, Menlo, monospace",
              }}
            >
              {showToken() ? status()?.token || "" : "••••••••••••••••"}
            </code>
            <button class="btn-secondary text-xs px-2 py-1" onClick={() => setShowToken(!showToken())}>
              {showToken() ? "隐藏" : "显示"}
            </button>
            <button
              class="btn-secondary text-xs px-2 py-1"
              onClick={() => copy(status()?.token || "", "令牌")}
            >
              复制
            </button>
            <button class="btn-ghost text-xs px-2 py-1" onClick={regen}>
              重新生成
            </button>
          </div>
          <div class="text-xs mt-2 leading-relaxed" style={{ color: s.muted }}>
            令牌存在本机（{`%APPDATA%\\PixVault\\bridge.json`} 一类的位置），扩展填一次就记住。
            换令牌只影响浏览器扩展，不动图库里的任何东西。
          </div>
        </div>

        {/* ── 安装扩展 ── */}
        <div class="text-sm mb-2" style={{ color: s.secondary }}>安装扩展</div>
        <div
          class="px-4 py-3 rounded-lg mb-2"
          style={{ background: s.bg, border: `1px solid ${s.border}` }}
        >
          <ol class="text-xs leading-relaxed pl-4" style={{ color: s.secondary }}>
            {installSteps.map((t) => (
              <li class="mb-1 last:mb-0" style={{ "list-style-type": "decimal" }}>{t}</li>
            ))}
          </ol>
          <div class="flex items-center gap-2 mt-3">
            <button class="btn-primary text-xs px-3 py-1.5" disabled={busy()} onClick={exportExt}>
              导出扩展到文件夹…
            </button>
            <Show when={exportedAt()}>
              <button
                class="btn-secondary text-xs px-2 py-1.5"
                onClick={() => api.revealPath(exportedAt()).catch(() => {})}
              >
                打开所在文件夹
              </button>
            </Show>
          </div>
          <Show when={exportedAt()}>
            <div class="text-2xs mt-2 break-all" style={{ color: s.muted }}>{exportedAt()}</div>
          </Show>
          <div class="text-xs mt-2 leading-relaxed" style={{ color: s.muted }}>
            装好后，网页里按 <b style={{ color: s.secondary }}>Alt + S</b> 打开采集面板，
            鼠标悬停在图片上会浮出采集按钮，右键图片也有「保存到 PixVault」。
            已经打开的网页需要刷新一次才会生效。
          </div>
        </div>

        {/* ── 最近摘取 ── */}
        <div class="text-sm mb-2 mt-4" style={{ color: s.secondary }}>最近摘取</div>
        <div
          class="rounded-lg overflow-hidden"
          style={{ background: s.bg, border: `1px solid ${s.border}` }}
        >
          <Show
            when={(status()?.recent?.length ?? 0) > 0}
            fallback={
              <div class="px-4 py-4 text-xs" style={{ color: s.muted }}>
                还没有从网页存过东西。装好扩展后随便打开一个图片页面试试。
              </div>
            }
          >
            <div class="max-h-48 overflow-y-auto">
              {status()!.recent.map((r) => (
                <div
                  class="flex items-center gap-2 px-3 py-1.5 text-xs border-b last:border-b-0"
                  style={{ "border-color": s.border }}
                  title={r.path}
                >
                  <span class="flex-1 truncate" style={{ color: s.primary }}>{r.filename}</span>
                  <span class="tabular-nums flex-shrink-0" style={{ color: s.muted }}>
                    {fmtBytes(r.size)}
                  </span>
                  <span class="flex-shrink-0 tabular-nums w-14 text-right" style={{ color: s.muted }}>
                    {r.at}
                  </span>
                </div>
              ))}
            </div>
          </Show>
        </div>

        <div class="text-xs mt-4 leading-relaxed" style={{ color: s.muted }}>
          数据流向：<span style={{ color: s.secondary }}>浏览器取字节 → 127.0.0.1 → 当前图库</span>。
          因为字节是浏览器去拿的，需要登录的图床、带防盗链的 CDN、canvas 画出来的图都能存下来；
          应用本身不联网，也不经过任何第三方服务器。
        </div>
      </div>
    </div>
  );
}
