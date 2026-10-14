import { createSignal, Show, For, createResource, createEffect, on, onMount, onCleanup } from "solid-js";
import type { JSX } from "solid-js";
import { api, COLOR_LABELS } from "@/lib/tauri-api";
import { activeLibraryId, useLibraryStore } from "@/stores/library";
import { useTagsStore } from "@/stores/tags";
import { useFilesStore } from "@/stores/files";
import { convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { thumbUrl, toDisplaySrc } from "@/lib/thumbs";

interface Props {
  /** 当前展示的文件；为 null 时面板显示「未选择文件 + 图库文件数」。 */
  filePath: string | null;
  onClose: () => void;
}

/** 可折叠分区（Pixcall 右侧信息区就是这种可折叠的分组）。 */
function Section(props: {
  title: string;
  open: boolean;
  onToggle: () => void;
  children: JSX.Element;
}) {
  return (
    <div class="info-section">
      <button
        class="w-full flex items-center gap-1.5 px-3 py-2 text-left"
        onClick={props.onToggle}
      >
        <span
          class="text-2xs transition-transform duration-150"
          style={{
            color: "var(--text-muted)",
            transform: props.open ? "rotate(90deg)" : "none",
          }}
        >▶</span>
        <span class="text-2xs font-semibold tracking-wide" style={{ color: "var(--text-secondary)" }}>
          {props.title}
        </span>
      </button>
      <Show when={props.open}>
        <div class="pb-2.5">{props.children}</div>
      </Show>
    </div>
  );
}

/** 一行属性：左标签 / 右值（Pixcall 的信息区排版）。 */
function Row(props: { label: string; title?: string; children: JSX.Element }) {
  return (
    <div class="info-row">
      <span class="info-row-label">{props.label}</span>
      <div class="info-row-value" title={props.title}>{props.children}</div>
    </div>
  );
}

/** 一行区块：标签在上、内容在下（标签 / 描述这类需要整行宽度的字段）。 */
function Block(props: { label: string; children: JSX.Element }) {
  return (
    <div class="px-3 py-1">
      <div class="text-2xs mb-1" style={{ color: "var(--text-muted)" }}>{props.label}</div>
      {props.children}
    </div>
  );
}

export function InfoPanel(props: Props) {
  const tags = useTagsStore();
  const files = useFilesStore();
  const library = useLibraryStore();

  const [notes, setNotes] = createSignal("");
  const [editingNotes, setEditingNotes] = createSignal(false);
  const [showTagPicker, setShowTagPicker] = createSignal(false);
  const [newTagName, setNewTagName] = createSignal("");
  const [busy, setBusy] = createSignal(false);

  const [openInfo, setOpenInfo] = createSignal(true);
  const [openMeta, setOpenMeta] = createSignal(true);
  /** 预览图加载失败时退回的 data URL（绕开 asset 协议；448px 缩略图够用）。 */
  const [previewDataUrl, setPreviewDataUrl] = createSignal<string | null>(null);
  /** 后端按需生成的 1600px 预览图路径；null = 还没就绪，先用更小的图顶着。 */
  const [previewUrl, setPreviewUrl] = createSignal<string | null>(null);
  /** 快速切换文件时，丢弃迟到的响应，避免预览栏串成上一张图。 */
  let previewToken = 0;

  const [fileInfo, { refetch }] = createResource(
    () => ({ libId: activeLibraryId(), path: props.filePath }),
    async ({ libId, path }) => {
      if (!libId || !path) return null;
      return api.getFileInfo(libId, path);
    }
  );

  onMount(() => {
    tags.loadTags();

    // 后台把 1600px 预览图生成好后推回来 —— 正好是当前这张才采用。
    let unlisten: (() => void) | undefined;
    listen<{ path: string; thumb: string | null }[]>("preview-ready", (event) => {
      const cur = props.filePath;
      if (!cur) return;
      const hit = event.payload?.find((it) => it.path === cur);
      if (hit?.thumb) {
        previewToken += 1; // 让仍在途的查询作废，事件结果优先
        setPreviewUrl(hit.thumb);
      }
    })
      .then((fn) => { unlisten = fn; })
      .catch(() => { /* 忽略 */ });
    onCleanup(() => unlisten?.());
  });

  // Reset transient edit state whenever the panel switches to another file
  createEffect(on(() => props.filePath, () => {
    setEditingNotes(false);
    setShowTagPicker(false);
    setNewTagName("");
    setNotes("");
    setPreviewDataUrl(null);
  }));

  /**
   * 取当前文件的 1600px 预览图。
   *
   * 后端只判断「文件在不在」，需要生成时丢进后台队列立刻回 `pending`，结果稍后
   * 由 `preview-ready` 事件推回来。**不做整库预热** —— 1600px 单张约 250 KB，
   * 2400 张全量生成就是 600 MB，而绝大多数图一辈子不会被点开。
   */
  createEffect(() => {
    const libId = activeLibraryId();
    const path = props.filePath;
    const token = ++previewToken;
    setPreviewUrl(null);
    if (!libId || !path) return;

    void (async () => {
      try {
        const res = await api.getPreviewPath(libId, path);
        if (token !== previewToken) return; // 已经切到别的图了
        if (res.status === "ready") setPreviewUrl(res.path);
      } catch {
        /* 失败就继续用缩略图 / 原图顶着 */
      }
    })();
  });

  /**
   * 预览栏的图片地址，按「越清晰越靠前」的优先级挑：
   *   生成的 1600px 预览图 → 网格缩略图（448px，秒出）→ 原图。
   * 原图放在最后：一张 12MP 原片解码要 36–50 MB 内存，只是过渡画面不值当。
   */
  const previewSrc = (filePath: string): string => {
    const dataUrl = previewDataUrl();
    if (dataUrl) return dataUrl;
    const full = previewUrl();
    if (full) return toDisplaySrc(full);
    const small = thumbUrl(filePath);
    if (small) return toDisplaySrc(small);
    return convertFileSrc(filePath);
  };

  /**
   * 预览图加载失败 → 放弃 1600px 预览图并退回 data URL（网格缩略图）。
   * 有些 WebView2 环境里 asset 协议（`http://asset.localhost/…`）加载不出来，
   * 那时预览区就是一块空白；直接读字节回来至少还能看到画面。
   */
  const handlePreviewError = async (e: Event) => {
    const el = e.currentTarget as HTMLImageElement;
    const path = props.filePath;
    const libId = activeLibraryId();
    void api.diagLog("preview", `load-error src=${el.src.slice(0, 90)}`);
    setPreviewUrl(null); // 这张的 1600px 预览加载不了，退回更小的图
    if (!path || !libId || previewDataUrl()) {
      el.style.visibility = "hidden";
      return;
    }
    const dataUrl = await api.readThumbDataUrl(libId, path);
    if (dataUrl) {
      void api.diagLog("preview", "data-url-ok");
      setPreviewDataUrl(dataUrl);
    } else {
      el.style.visibility = "hidden";
    }
  };

  const formatSize = (bytes: number) => {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  };

  /** Timestamps come from SQLite as "YYYY-MM-DD HH:MM:SS" in UTC, whereas a
   *  freshly indexed entry may still be an ISO string — normalise both. */
  const parseTime = (value?: string | null): Date | null => {
    if (!value) return null;
    const iso = /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/.test(value)
      ? value.replace(" ", "T") + "Z"
      : value;
    const d = new Date(iso);
    return Number.isNaN(d.getTime()) ? null : d;
  };

  const fmtTime = (value?: string | null): string => {
    const d = parseTime(value);
    if (!d) return "—";
    const pad = (n: number) => String(n).padStart(2, "0");
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
  };

  /** 所在文件夹（去掉最后一段文件名）。 */
  const parentDir = (path: string) => path.replace(/[\\/][^\\/]*$/, "") || path;

  const handleSaveNotes = async () => {
    const libId = activeLibraryId();
    if (!libId || !props.filePath) return;
    setBusy(true);
    try {
      await api.updateFileNotes(libId, props.filePath, notes());
      await refetch();
      setEditingNotes(false);
    } finally {
      setBusy(false);
    }
  };

  const handleRating = async (star: number) => {
    const libId = activeLibraryId();
    if (!libId || !props.filePath) return;
    const current = fileInfo()?.rating || 0;
    await api.updateFileRating(libId, props.filePath, current >= star ? 0 : star);
    await refetch();
  };

  const handleFavorite = async (favorite: boolean) => {
    if (!props.filePath) return;
    await files.setFavorite(props.filePath, favorite);
    await refetch();
  };

  const handleColor = async (color: string | null) => {
    if (!props.filePath) return;
    await files.setColor(props.filePath, color);
    await refetch();
  };

  const handleAddTag = async (tagId: number) => {
    const libId = activeLibraryId();
    if (!libId || !props.filePath) return;
    setBusy(true);
    try {
      await api.addTagToFile(libId, props.filePath, tagId);
      await refetch();
      await files.updateFileTags(props.filePath);
    } finally {
      setBusy(false);
    }
  };

  const handleRemoveTag = async (tagId: number) => {
    const libId = activeLibraryId();
    if (!libId || !props.filePath) return;
    setBusy(true);
    try {
      await api.removeTagFromFile(libId, props.filePath, tagId);
      await refetch();
      await files.updateFileTags(props.filePath);
    } finally {
      setBusy(false);
    }
  };

  const handleCreateAndAddTag = async () => {
    const name = newTagName().trim();
    if (!name) return;
    const created = await tags.createTag(name);
    if (created) await handleAddTag(created.id);
    setNewTagName("");
  };

  const s = {
    text: "var(--text-primary)",
    muted: "var(--text-muted)",
    accent: "var(--accent)",
    border: "var(--border-color)",
    tertiary: "var(--bg-tertiary)",
    hover: "var(--bg-hover)",
  };

  const libFileCount = () => library.activeLibrary()?.file_count ?? files.totalFiles();

  return (
    /* 占位式布局：面板是 flex 兄弟节点、宽度固定，网格区域随之变窄，
       列数由 auto-fill 自动从 6 列收成 5 列（不再浮层遮挡图片）。 */
    <aside class="info-panel w-[19rem] flex-shrink-0 flex flex-col overflow-hidden animate-slide-in-right">
      {/* 头部 */}
      <div class="flex items-center h-9 pl-3 pr-1.5 flex-shrink-0" style={{ borderBottom: `1px solid ${s.border}` }}>
        <span class="text-xs font-semibold" style={{ color: s.text }}>文件信息</span>
        <div class="flex-1" />
        <button class="btn-ghost text-2xs px-1.5 py-0.5" title="展开／折叠全部分区"
          onClick={() => {
            const next = !(openInfo() && openMeta());
            setOpenInfo(next);
            setOpenMeta(next);
          }}>⇕</button>
        <button class="btn-ghost text-xs px-1.5 py-0.5" title="关闭" onClick={props.onClose}>✕</button>
      </div>

      {/* 未选择文件：只显示图库文件数量（Pixcall 也是如此） */}
      <Show when={props.filePath} fallback={
        <div class="flex-1 flex flex-col items-center justify-center gap-1.5 px-6 text-center">
          <div class="text-3xl opacity-50">🖼️</div>
          <p class="text-xs" style={{ color: s.muted }}>未选择文件</p>
          <p class="text-sm font-medium" style={{ color: s.text }}>
            当前图库 {libFileCount()} 个文件
          </p>
          <Show when={files.selectedFiles().size > 0}>
            <p class="text-2xs" style={{ color: s.accent }}>已选 {files.selectedFiles().size} 个</p>
          </Show>
        </div>
      }>
        {(path) => (
          <div class="flex-1 overflow-y-auto">
            {/* 预览 */}
            <div class="flex items-center justify-center flex-shrink-0"
              style={{ background: s.tertiary, height: "13rem" }}>
              <img src={previewSrc(path())} alt=""
                class="max-w-full max-h-full object-contain" draggable={false}
                onError={handlePreviewError} />
            </div>

            <Show when={fileInfo()} fallback={
              <div class="p-4 text-xs" style={{ color: s.muted }}>加载中...</div>
            }>
              {(info) => (
                <>
                  {/* 文件信息 */}
                  <Section title="文件信息" open={openInfo()} onToggle={() => setOpenInfo(!openInfo())}>
                    <Row label="文件名" title={info().filename}>
                      <span class="allow-select">{info().filename}</span>
                    </Row>

                    <Block label="标签">
                      <div class="flex flex-wrap gap-1">
                        <Show when={info().tags.length > 0} fallback={<span class="text-2xs" style={{ color: s.muted }}>暂无标签</span>}>
                          <For each={info().tags}>
                            {(tag) => (
                              <span class="tag-pill text-white" style={{ background: tag.color || "#6c757d" }}>
                                {tag.name}
                                <button class="ml-1 text-white/60 hover:text-white"
                                  title="移除标签"
                                  onClick={() => handleRemoveTag(tag.id)}>×</button>
                              </span>
                            )}
                          </For>
                        </Show>
                        <button class="tag-pill" style={{ background: s.tertiary, color: s.muted }}
                          onClick={() => setShowTagPicker(!showTagPicker())}>
                          {showTagPicker() ? "收起" : "+ 标签"}
                        </button>
                      </div>

                      <Show when={showTagPicker()}>
                        <div class="mt-1.5 rounded-md overflow-hidden" style={{ border: `1px solid ${s.border}`, background: s.tertiary }}>
                          <div class="max-h-40 overflow-y-auto py-1">
                            <Show when={tags.tags().length > 0} fallback={
                              <p class="px-2 py-1 text-2xs" style={{ color: s.muted }}>还没有标签，先在下方新建</p>
                            }>
                              <For each={tags.tags()}>
                                {(tag) => {
                                  const applied = () => info().tags.some((t) => t.id === tag.id);
                                  return (
                                    <div class="flex items-center gap-2 px-2 py-1 cursor-pointer text-xs transition-opacity hover:opacity-80"
                                      style={{ color: applied() ? s.accent : s.text }}
                                      onClick={() => applied() ? handleRemoveTag(tag.id) : handleAddTag(tag.id)}>
                                      <span class="w-2.5 h-2.5 rounded-full flex-shrink-0" style={{ background: tag.color || s.muted }} />
                                      <span class="truncate flex-1">{tag.name}</span>
                                      <Show when={applied()}>
                                        <span style={{ color: s.accent }}>✓</span>
                                      </Show>
                                    </div>
                                  );
                                }}
                              </For>
                            </Show>
                          </div>
                          <div class="flex gap-1 p-1.5" style={{ borderTop: `1px solid ${s.border}` }}>
                            <input class="input flex-1 text-xs py-1" placeholder="新建标签..." value={newTagName()}
                              onInput={(e) => setNewTagName(e.currentTarget.value)}
                              onKeyDown={(e) => { if (e.key === "Enter") handleCreateAndAddTag(); }} />
                            <button class="btn-primary text-xs px-2" onClick={handleCreateAndAddTag}>新建</button>
                          </div>
                        </div>
                      </Show>
                    </Block>

                    <Block label="描述">
                      <Show when={editingNotes()} fallback={
                        <div class="text-xs min-h-[1.75rem] cursor-text p-1 rounded whitespace-pre-wrap break-words"
                          style={{ background: s.tertiary, color: info().notes ? s.text : s.muted }}
                          onClick={() => { setNotes(info().notes || ""); setEditingNotes(true); }}>
                          {info().notes || "点击添加描述..."}
                        </div>
                      }>
                        <textarea class="input text-xs min-h-[4rem] resize-none" value={notes()}
                          onInput={(e) => setNotes(e.currentTarget.value)}
                          onKeyDown={(e) => { if (e.key === "Enter" && e.ctrlKey) handleSaveNotes(); if (e.key === "Escape") setEditingNotes(false); }} />
                        <div class="flex gap-1 mt-1 items-center">
                          <button class="btn-primary text-2xs px-2 py-0.5" disabled={busy()} onClick={handleSaveNotes}>保存</button>
                          <button class="btn-ghost text-2xs" onClick={() => setEditingNotes(false)}>取消</button>
                          <span class="text-2xs ml-auto" style={{ color: s.muted }}>Ctrl+Enter</span>
                        </div>
                      </Show>
                    </Block>

                    <Block label="评分">
                      <div class="flex gap-0.5">
                        {[1, 2, 3, 4, 5].map((star) => (
                          <button class="text-base leading-none transition-colors"
                            style={{ color: (info().rating || 0) >= star ? "var(--star-color)" : s.muted }}
                            onClick={() => handleRating(star)}>★</button>
                        ))}
                      </div>
                    </Block>

                    <Row label="收藏">
                      <button class="text-sm leading-none transition-colors"
                        style={{ color: info().favorite ? "#ff6b6b" : s.muted }}
                        onClick={() => handleFavorite(!info().favorite)}>
                        {info().favorite ? "♥ 已收藏" : "♡ 收藏"}
                      </button>
                    </Row>

                    <Block label="颜色标记">
                      <div class="flex gap-1.5 items-center">
                        <For each={COLOR_LABELS}>
                          {(c) => (
                            <button class="w-4 h-4 rounded-full transition-transform hover:scale-110"
                              classList={{ "ring-2 ring-offset-1": info().color_label === c.name }}
                              style={{ background: c.hex, "ring-color": "var(--accent)" }}
                              title={c.name}
                              onClick={() => handleColor(info().color_label === c.name ? null : c.name)} />
                          )}
                        </For>
                        <Show when={info().color_label}>
                          <button class="text-2xs ml-1" style={{ color: s.muted }}
                            onClick={() => handleColor(null)}>清除</button>
                        </Show>
                      </div>
                    </Block>

                    <Row label="所在文件夹" title={parentDir(info().path)}>
                      <span class="allow-select">{parentDir(info().path)}</span>
                    </Row>
                  </Section>

                  {/* 基本信息 */}
                  <Section title="基本信息" open={openMeta()} onToggle={() => setOpenMeta(!openMeta())}>
                    <Row label="尺寸">{info().width && info().height ? `${info().width} × ${info().height}` : "未知"}</Row>
                    <Row label="类型">{info().extension.toUpperCase()}</Row>
                    <Row label="大小">{formatSize(info().size_bytes)}</Row>
                    <Row label="创建时间"><span class="tabular-nums">{fmtTime(info().ctime)}</span></Row>
                    <Row label="修改时间"><span class="tabular-nums">{fmtTime(info().mtime)}</span></Row>
                    <Row label="导入时间"><span class="tabular-nums">{fmtTime(info().created_at)}</span></Row>
                    <Row label="更新时间"><span class="tabular-nums">{fmtTime(info().updated_at)}</span></Row>
                    <Row label="完整路径" title={info().path}>
                      <span class="text-2xs break-all allow-select" style={{ color: s.muted }}>{info().path}</span>
                    </Row>
                  </Section>
                </>
              )}
            </Show>
          </div>
        )}
      </Show>

      {/* 底部操作 */}
      <div class="flex gap-1.5 p-2 flex-shrink-0" style={{ borderTop: `1px solid ${s.border}` }}>
        <button class="btn-secondary text-2xs flex-1 py-1" onClick={() => props.filePath && api.openInExplorer(props.filePath)}>
          📂 显示位置
        </button>
        <button class="btn-secondary text-2xs flex-1 py-1" onClick={() => props.filePath && api.openFile(props.filePath)}>
          🔗 打开方式
        </button>
      </div>
    </aside>
  );
}
