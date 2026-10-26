import { createSignal, onMount, Show } from "solid-js";
import type { JSX } from "solid-js";
import { api, type GpuMode, type GpuStatus, type ImportSummary } from "@/lib/tauri-api";
import { useLibraryStore, warmForLibrary } from "@/stores/library";
import { useUIStore } from "@/stores/ui";
import { CircleProgress } from "./CircleProgress";
import { open } from "@tauri-apps/plugin-dialog";
import { useTheme } from "@/stores/theme";
import { IconArrowRight, IconClose, IconEagle, IconPackage } from "@/components/icons";

interface Props {
  onClose: () => void;
  onImported: () => void;
}

export function SettingsModal(props: Props) {
  const theme = useTheme();
  const library = useLibraryStore();
  const ui = useUIStore();
  const [busy, setBusy] = createSignal(false);
  const [result, setResult] = createSignal<ImportSummary | null>(null);
  const [error, setError] = createSignal("");
  const [gpu, setGpu] = createSignal<GpuStatus | null>(null);
  const [gpuBusy, setGpuBusy] = createSignal(false);

  onMount(async () => {
    try {
      setGpu(await api.getGpuStatus());
    } catch {
      /* 拿不到就按「未知」渲染，不影响其它设置项 */
    }
  });

  /** 切换渲染方式。改完立刻落盘，但要重启才生效（WebView2 参数只能在建窗时传）。 */
  const pickGpuMode = async (mode: GpuMode) => {
    setGpuBusy(true);
    try {
      setGpu(await api.setGpuMode(mode));
      ui.showToast("渲染方式已保存，重启应用后生效", "success");
    } catch (e) {
      ui.showToast(`切换失败：${String(e)}`, "error");
    } finally {
      setGpuBusy(false);
    }
  };

  const GPU_LABELS: { value: GpuMode; label: string }[] = [
    { value: "auto", label: "自动" },
    { value: "hardware", label: "硬件加速" },
    { value: "software", label: "软件渲染" },
  ];

  /** 当前活动图库的预热进度（可能没有 —— 表示已经补完或还没开始）。 */
  const activeWarm = () => {
    const id = library.activeLibraryId();
    return id ? warmForLibrary(id) : undefined;
  };

  /** 手动补齐当前图库的缩略图（不受自动预热开关影响）。 */
  const buildMissing = async () => {
    const id = library.activeLibraryId();
    if (!id) return;
    try {
      const queued = await api.buildMissingThumbnails(id);
      ui.showToast(
        queued > 0 ? `已排队生成 ${queued} 张缩略图` : "缩略图已经是齐的",
        queued > 0 ? "success" : "info",
      );
    } catch (e) {
      ui.showToast(`补齐失败：${String(e)}`, "error");
    }
  };

  const s = {
    primary: "var(--text-primary)",
    secondary: "var(--text-secondary)",
    muted: "var(--text-muted)",
    bg: "var(--bg-secondary)",
    tertiary: "var(--bg-tertiary)",
    border: "var(--border-color)",
    accent: "var(--accent)",
  };

  const pickAndImport = async (kind: "eagle" | "pixcall") => {
    setBusy(true);
    setError("");
    setResult(null);
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: kind === "eagle" ? "选择 Eagle 资源库 (.library 文件夹)" : "选择 Pixcall 资源库文件夹",
      });
      if (!selected) {
        setBusy(false);
        return;
      }

      const summary =
        kind === "eagle"
          ? await api.importEagleLibrary(selected as string)
          : await api.importPixcallLibrary(selected as string);

      // Import created a dedicated library — add it to the sidebar list.
      library.setLibraries((prev) => {
        if (prev.find((l) => l.id === summary.library.id)) return prev;
        return [...prev, summary.library];
      });
      library.setActiveLibraryId(summary.library.id);

      setResult(summary);
      props.onImported();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div class="fixed inset-0 z-50 flex items-center justify-center" style={{ background: "rgba(0,0,0,0.5)" }}>
      <div
        class="glass-modal rounded-xl p-6 w-[520px] max-w-[90vw] animate-scale-in"
        style={{ color: s.primary }}
      >
        <div class="flex items-center justify-between mb-5">
          <h2 class="text-lg font-medium" style={{ color: s.primary }}>设置</h2>
          <button class="btn-ghost text-sm px-2 flex" onClick={props.onClose} title="关闭">
            <IconClose size={15} />
          </button>
        </div>

        {/* ── 缩略图 ─────────────────────────────────────────── */}
        <div class="text-sm mb-3" style={{ color: s.secondary }}>缩略图</div>

        <div class="px-4 py-3 rounded-lg mb-5" style={{ background: s.tertiary, border: `1px solid ${s.border}` }}>
          <label class="flex items-start gap-3 cursor-pointer">
            <input
              type="checkbox"
              class="mt-0.5"
              checked={ui.prewarmEnabled()}
              onChange={(e) => ui.setPrewarmEnabled(e.currentTarget.checked)}
            />
            <div class="flex-1 min-w-0">
              <div class="text-sm" style={{ color: s.primary }}>打开图库时在后台生成缩略图</div>
              <div class="text-xs mt-0.5 leading-relaxed" style={{ color: s.muted }}>
                打开、导入或恢复图库时，自动把还没有缩略图的图片排进后台队列慢慢补齐，
                进度以圆环显示在图库名旁边。关闭后只有滚动到某张图时才按需生成 ——
                省电省内存，但快速下滑时更容易看到模糊占位。
              </div>
            </div>
          </label>

          <div class="mt-3 flex items-center gap-3">
            <button
              class="btn-secondary text-xs px-2 py-1"
              disabled={!library.activeLibraryId()}
              onClick={buildMissing}
            >立即补齐当前图库</button>

            <Show when={activeWarm()}>
              {(w) => (
                <span class="flex items-center gap-1.5 text-xs" style={{ color: s.muted }}>
                  <CircleProgress value={w().done} max={w().total} size={18} />
                  <span class="tabular-nums" style={{ color: s.accent }}>{w().done}</span>
                  <span>/ {w().total}</span>
                </span>
              )}
            </Show>
          </div>
        </div>

        {/* ── 渲染 ─────────────────────────────────────────── */}
        <div class="text-sm mb-3" style={{ color: s.secondary }}>渲染</div>

        <div class="px-4 py-3 rounded-lg mb-5" style={{ background: s.tertiary, border: `1px solid ${s.border}` }}>
          <div class="flex items-center gap-2">
            <span class="text-sm flex-1" style={{ color: s.primary }}>硬件加速（GPU 渲染）</span>
            <Show when={gpu()}>
              {(g) => (
                <span class="text-xs px-1.5 py-0.5 rounded"
                  style={{
                    background: g().software_rendering ? "var(--bg-secondary)" : "var(--accent-bg)",
                    color: g().software_rendering ? s.muted : s.accent,
                  }}
                >
                  当前：{g().software_rendering ? "软件渲染" : "硬件渲染"}
                </span>
              )}
            </Show>
          </div>

          <div class="text-xs mt-0.5 leading-relaxed" style={{ color: s.muted }}>
            自动：GPU 能起就用，崩过一次就退回软件渲染并记住；硬件加速：强制再试一次
            （仍然保留崩溃保护，真崩了下次会自动退回，不会卡在白屏）；软件渲染：完全不用
            GPU —— 省电，也能避开某些显卡驱动的画面异常。
          </div>

          <div class="mt-3 flex items-center gap-2">
            {GPU_LABELS.map((m) => (
              <button
                class="btn-secondary text-xs px-2.5 py-1"
                disabled={gpuBusy() || !gpu()}
                style={{
                  background: gpu()?.mode === m.value ? s.accent : undefined,
                  color: gpu()?.mode === m.value ? "#fff" : undefined,
                }}
                onClick={() => pickGpuMode(m.value)}
              >{m.label}</button>
            ))}
          </div>

          <Show when={gpu()?.crashed_last_run && gpu()?.mode === "auto"}>
            <div class="mt-2 text-xs leading-relaxed" style={{ color: "var(--text-secondary)" }}>
              上次启动没有正常渲染完成，已自动改用软件渲染。把显卡驱动或系统里的
              Microsoft Edge WebView2 Runtime 修好之后，可以再选「硬件加速」试一次。
            </div>
          </Show>

          <div class="mt-2 text-xs" style={{ color: s.muted }}>改动需要重启应用才生效。</div>
        </div>

        <div class="text-sm mb-4" style={{ color: s.secondary }}>从其它素材管理软件导入资源库</div>

        <div class="flex flex-col gap-3">
          <ImportRow
            icon={<IconEagle size={20} />}
            title="导入 Eagle 资源库"
            desc="读取 .library 内的 metadata.json，保留文件夹层级（映射为标签）、标签、备注、评分与来源链接"
            busy={busy()}
            onPick={() => pickAndImport("eagle")}
          />
          <ImportRow
            icon={<IconPackage size={20} />}
            title="导入 Pixcall 资源库"
            desc="读取 .pixcall/database/main.db，保留文件夹层级、标签、描述、评分与来源网址"
            busy={busy()}
            onPick={() => pickAndImport("pixcall")}
          />
        </div>

        <Show when={busy()}>
          <div class="text-sm mt-4" style={{ color: s.muted }}>正在导入，请稍候…</div>
        </Show>

        <Show when={error()}>
          <div class="text-sm mt-4 px-3 py-2 rounded-lg" style={{ background: "var(--danger)", color: "#fff" }}>
            {error()}
          </div>
        </Show>

        <Show when={result()}>
          <div class="text-sm mt-4 px-3 py-3 rounded-lg" style={{ background: s.tertiary, border: `1px solid ${s.border}` }}>
            <div style={{ color: s.primary }}>导入完成 · 已创建新库「{result()!.library.name}」</div>
            <div class="mt-1 flex flex-wrap gap-x-4 gap-y-1" style={{ color: s.secondary }}>
              <span>文件 {result()!.imported_files} / {result()!.total_items}</span>
              <span>新建标签 {result()!.created_tags}</span>
            </div>
            <Show when={result()!.errors.length > 0}>
              <div class="mt-1" style={{ color: "var(--danger)" }}>{result()!.errors.length} 个文件导入失败</div>
            </Show>
          </div>
        </Show>

        <div class="text-xs mt-5" style={{ color: s.muted }}>
          导入会新建一个独立的图片库（以源库命名），不会与当前库的图片合并，也不会复制或移动原始文件。
        </div>
      </div>
    </div>
  );
}

interface RowProps {
  icon: JSX.Element;
  title: string;
  desc: string;
  busy: boolean;
  onPick: () => void;
}

function ImportRow(props: RowProps) {
  return (
    <div
      class="flex items-start gap-3 px-4 py-3 rounded-lg transition-colors cursor-pointer"
      style={{
        background: "var(--bg-tertiary)",
        border: "1px solid var(--border-color)",
      }}
      onClick={() => !props.busy && props.onPick()}
    >
      <span class="mt-0.5" style={{ color: "var(--text-primary)" }}>{props.icon}</span>
      <div class="flex-1 min-w-0">
        <div class="text-sm font-medium" style={{ color: "var(--text-primary)" }}>{props.title}</div>
        <div class="text-xs mt-0.5 leading-relaxed" style={{ color: "var(--text-muted)" }}>{props.desc}</div>
      </div>
      <span class="flex mt-1" style={{ color: "var(--accent)" }}><IconArrowRight size={15} /></span>
    </div>
  );
}
