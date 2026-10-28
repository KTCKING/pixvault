import { createSignal, onMount, onCleanup, Show } from "solid-js";
import type { JSX } from "solid-js";
import { api, type GpuMode, type GpuStatus, type ImportSummary, type PackProbe, type PackProgress } from "@/lib/tauri-api";
import { useLibraryStore, warmForLibrary } from "@/stores/library";
import { useUIStore } from "@/stores/ui";
import { CircleProgress } from "./CircleProgress";
import { open } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import { useTheme } from "@/stores/theme";
import { IconArrowRight, IconClose, IconDatabase, IconEagle, IconPackage, IconPuzzle } from "@/components/icons";
import { ExtensionModal } from "./ExtensionModal";

interface Props {
  onClose: () => void;
  onImported: () => void;
}

/** 把字节数写成人类能读的大小。 */
const fmtBytes = (n: number): string => {
  if (!n) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  let v = n;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v >= 100 || i === 0 ? Math.round(v) : v.toFixed(1)} ${units[i]}`;
};

export function SettingsModal(props: Props) {
  const theme = useTheme();
  const library = useLibraryStore();
  const ui = useUIStore();
  const [busy, setBusy] = createSignal(false);
  const [result, setResult] = createSignal<ImportSummary | null>(null);
  const [error, setError] = createSignal("");
  const [gpu, setGpu] = createSignal<GpuStatus | null>(null);
  const [gpuBusy, setGpuBusy] = createSignal(false);
  /** 选完 .eaglepack 后先给出的统计，等用户确认再解压 */
  const [pendingPack, setPendingPack] = createSignal<{ path: string; probe: PackProbe } | null>(null);
  const [packProgress, setPackProgress] = createSignal<PackProgress | null>(null);
  /** 浏览器扩展子面板（设置内容已经够长了，单独一层更好读） */
  const [extOpen, setExtOpen] = createSignal(false);

  onMount(async () => {
    try {
      setGpu(await api.getGpuStatus());
    } catch {
      /* 拿不到就按「未知」渲染，不影响其它设置项 */
    }
  });

  // 解压进度（几千个条目、几个 GB 的包都靠它给反馈）
  onMount(async () => {
    const un = await listen<PackProgress>("eaglepack-progress", (e) => setPackProgress(e.payload));
    onCleanup(un);
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

  /** 第一步：选 .eaglepack，先只读中央目录给出统计，不解压。 */
  const chooseEaglepack = async () => {
    setError("");
    setResult(null);
    setPendingPack(null);
    try {
      const picked = await open({
        multiple: false,
        title: "选择 Eagle 素材包",
        filters: [{ name: "Eagle 素材包", extensions: ["eaglepack", "zip"] }],
      });
      if (!picked) return;

      setBusy(true);
      const probe = await api.probeEaglepack(picked as string);
      if (probe.layout === "unknown") {
        throw new Error(
          "这个包里既没有 pack.json，也没有 Eagle 库的 metadata.json + images 目录，无法识别。",
        );
      }
      setPendingPack({ path: picked as string, probe });
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  /** 第二步：选目标文件夹（会作为新图库的根），然后流式解压。 */
  const startEaglepack = async () => {
    const pending = pendingPack();
    if (!pending) return;

    const dest = await open({
      directory: true,
      multiple: false,
      title: "选择解压到哪里（需空文件夹，它会成为新图库的根）",
    });
    if (!dest) return;

    setBusy(true);
    setError("");
    setResult(null);
    setPackProgress(null);
    try {
      const summary = await api.importEaglepack(pending.path, dest as string);

      library.setLibraries((prev) => {
        if (prev.find((l) => l.id === summary.library.id)) return prev;
        return [...prev, summary.library];
      });
      library.setActiveLibraryId(summary.library.id);

      setResult(summary);
      setPendingPack(null);
      props.onImported();
    } catch (e) {
      const msg = String(e);
      setError(
        msg.includes("__canceled__")
          ? "已取消导入，解压出的半成品已清理。"
          : msg,
      );
    } finally {
      setBusy(false);
      setPackProgress(null);
    }
  };

  const cancelPack = () => {
    void api.cancelEaglepackImport();
  };

  return (
    <div class="fixed inset-0 z-50 flex items-center justify-center" style={{ background: "rgba(0,0,0,0.5)" }}>
      <div
        class="glass-modal rounded-xl p-6 w-[520px] max-w-[90vw] max-h-[88vh] overflow-y-auto animate-scale-in"
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

        <div class="text-sm mb-3" style={{ color: s.secondary }}>浏览器扩展</div>

        <div class="flex flex-col gap-3 mb-5">
          <ImportRow
            icon={<IconPuzzle size={20} />}
            title="从网页摘取图片和视频"
            desc="装一个浏览器扩展，把网页上的图片 / 视频一键存进当前图库：面板多选、悬停速存、右键保存都支持"
            busy={false}
            onPick={() => setExtOpen(true)}
          />
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
            icon={<IconDatabase size={20} />}
            title="导入 Pixcall 资源库"
            desc="读取 .pixcall/database/main.db，保留文件夹层级、标签、描述、评分与来源网址"
            busy={busy()}
            onPick={() => pickAndImport("pixcall")}
          />
          <ImportRow
            icon={<IconPackage size={20} />}
            title="导入 Eagle 素材包（.eaglepack）"
            desc="直接选 .eaglepack 文件：流式解压到目标文件夹并作为新图库打开，保留文件夹层级、标签、备注、评分与来源链接"
            busy={busy()}
            onPick={chooseEaglepack}
          />
        </div>

        {/* 选完包先给统计，让用户知道要占多少盘、里面有多少东西 */}
        <Show when={pendingPack()}>
          <div
            class="mt-3 px-3 py-3 rounded-lg"
            style={{ background: s.tertiary, border: `1px solid ${s.border}` }}
          >
            <div class="text-xs" style={{ color: s.primary }}>
              {pendingPack()!.probe.item_count} 个条目 ·{" "}
              {pendingPack()!.probe.folder_count} 个文件夹 · 解压后约{" "}
              {fmtBytes(pendingPack()!.probe.unpacked_bytes)}
            </div>
            <div class="text-2xs mt-1" style={{ color: s.muted }}>
              布局：{pendingPack()!.probe.layout === "pack" ? "Eagle 素材包（pack.json）" : "Eagle 库（metadata.json）"}
              。目标文件夹需要预留上面这个体积的空闲空间。
            </div>
            <div class="flex gap-2 mt-2">
              <button class="btn-primary text-xs px-3 py-1" onClick={startEaglepack}>
                选择目标文件夹并开始
              </button>
              <button class="btn-secondary text-xs px-3 py-1" onClick={() => setPendingPack(null)}>
                取消
              </button>
            </div>
          </div>
        </Show>

        {/* 解压进度：几千个条目、几个 GB 的包都靠它给反馈 */}
        <Show when={packProgress()}>
          {(p) => (
            <div class="mt-4">
              <div class="flex items-center justify-between text-xs mb-1.5" style={{ color: s.secondary }}>
                <span>
                  {p().phase === "parse"
                    ? "正在解析元数据…"
                    : p().phase === "organize"
                      ? `正在按文件夹重组 ${p().done} / ${p().total}`
                      : `正在解压 ${p().done} / ${p().total}`}
                </span>
                {/* 重组阶段搬的是同一卷上的文件，几毫秒一个，没有可停的点 */}
                <Show when={p().phase !== "organize"}>
                  <button class="btn-ghost text-xs px-2 py-0.5" onClick={cancelPack}>
                    取消
                  </button>
                </Show>
              </div>
              <div class="h-1.5 rounded-full overflow-hidden" style={{ background: s.tertiary }}>
                <div
                  class="h-full transition-all"
                  style={{
                    width: `${p().total ? Math.min(100, Math.round((p().done / p().total) * 100)) : 0}%`,
                    background: s.accent,
                  }}
                />
              </div>
              <Show when={p().current}>
                <div class="text-2xs mt-1 truncate" style={{ color: s.muted }} title={p().current}>
                  {p().current}
                </div>
              </Show>
            </div>
          )}
        </Show>

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
              <Show when={(result()!.reorganized ?? 0) > 0}>
                <span>按文件夹重组 {result()!.reorganized}</span>
              </Show>
              <Show when={(result()!.renamed ?? 0) > 0}>
                <span>重名改名 {result()!.renamed}</span>
              </Show>
              <Show when={(result()!.skipped ?? 0) > 0}>
                <span>跳过 {result()!.skipped}</span>
              </Show>
            </div>
            <Show when={result()!.errors.length > 0}>
              <div class="mt-1" style={{ color: "var(--danger)" }}>{result()!.errors.length} 个文件导入失败</div>
            </Show>
          </div>
        </Show>

        <div class="text-xs mt-5 leading-relaxed" style={{ color: s.muted }}>
          导入会新建一个独立的图片库（以源库命名），不会与当前库的图片合并。Eagle 库与 Pixcall
          库是原地读取，不复制文件；<span style={{ color: s.secondary }}>eaglepack 不同</span>
          ——包里的素材必须解压到硬盘上，所以会让你先选一个空文件夹，它就成为新图库的根，之后随时可以删掉那个文件夹。解压出来的文件会
          <span style={{ color: s.secondary }}>按 Eagle 里的文件夹层级摆好</span>
          ，&lt;条目ID&gt;.info 这类壳目录直接清掉，同名文件自动加 (2) 序号。
        </div>
      </div>

      {/* 扩展面板叠在设置之上：它自己是一层 fixed 遮罩 */}
      <Show when={extOpen()}>
        <ExtensionModal onClose={() => setExtOpen(false)} />
      </Show>
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
