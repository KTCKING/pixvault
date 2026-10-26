import { createSignal } from "solid-js";
import { api } from "@/lib/tauri-api";

export type ViewMode = "grid" | "list" | "masonry";
export type SidebarTab = "folders" | "tags" | "smart-folders" | "search";

// ── 打开图库时后台补齐缩略图（默认开） ─────────────
//
// 这是"预热"：打开 / 导入 / 恢复图库时，后端会把缺缩略图的文件整批排进后台
// 队列，进度以圆环显示在图库名旁边。关掉它不会影响"滚动到哪张生成哪张"的
// 按需路径，也不影响设置里的手动补齐。
const PREWARM_KEY = "pixvault.prewarm-enabled";

function readPrewarmFlag(): boolean {
  try {
    const v = localStorage.getItem(PREWARM_KEY);
    return v === null ? true : v === "1";
  } catch {
    return true;
  }
}

const [prewarmEnabled, setPrewarmEnabledSignal] = createSignal<boolean>(readPrewarmFlag());

function setPrewarmEnabled(on: boolean) {
  setPrewarmEnabledSignal(on);
  try {
    localStorage.setItem(PREWARM_KEY, on ? "1" : "0");
  } catch {
    /* 忽略：无痕模式等场景下写不进去也不影响本次会话 */
  }
  api.setPrewarmEnabled(on).catch(() => { /* 后端未就绪时忽略 */ });
}

/** 启动时把本地偏好同步给后端（后端默认也是开，这里只是对齐两者）。 */
async function syncPrewarmToBackend() {
  try {
    await api.setPrewarmEnabled(prewarmEnabled());
  } catch {
    /* 忽略 */
  }
}

const [viewMode, setViewMode] = createSignal<ViewMode>("grid");
const [thumbnailSize, setThumbnailSize] = createSignal(220);
const [sidebarOpen, setSidebarOpen] = createSignal(true);
const [sidebarTab, setSidebarTab] = createSignal<SidebarTab>("folders");
const [searchQuery, setSearchQuery] = createSignal("");
const [showSearch, setShowSearch] = createSignal(false);
const [infoPanelOpen, setInfoPanelOpen] = createSignal(false);
const [infoPanelFile, setInfoPanelFile] = createSignal<string | null>(null);
const [isIndexing, setIsIndexing] = createSignal(false);
const [showSettings, setShowSettings] = createSignal(false);
const [showTrash, setShowTrash] = createSignal(false);
/** True while an image is being dragged over the trash drop target. */
const [trashHover, setTrashHover] = createSignal(false);

export type ContextMenuTarget =
  | { kind: "files"; paths: string[] }
  | { kind: "library"; libraryId: string }
  | { kind: "trash"; paths: string[] }
  | { kind: "sidebar" };

const [contextMenu, setContextMenu] = createSignal<{
  x: number;
  y: number;
  target: ContextMenuTarget;
} | null>(null);

export type ToastType = "success" | "info" | "error";
export interface ToastItem {
  id: number;
  message: string;
  type: ToastType;
}

const [toasts, setToasts] = createSignal<ToastItem[]>([]);
let toastSeq = 0;

function showToast(message: string, type: ToastType = "success", duration = 2200) {
  const id = ++toastSeq;
  setToasts((prev) => [...prev, { id, message, type }]);
  setTimeout(() => {
    setToasts((prev) => prev.filter((t) => t.id !== id));
  }, duration);
}

function dismissToast(id: number) {
  setToasts((prev) => prev.filter((t) => t.id !== id));
}

// ── Modal dialogs (confirm / prompt) ─────────────
// The Tauri webview does not reliably support window.confirm / window.prompt
// (they can be silently ignored), so every confirmation goes through this
// in-app dialog instead.
export interface DialogAction {
  label: string;
  danger?: boolean;
  onClick: (value: string) => void | Promise<void>;
}
export interface DialogState {
  title?: string;
  message?: string;
  /** Present => render a text input (prompt mode). */
  input?: { defaultValue?: string; placeholder?: string };
  /** Last action is rendered as the primary button. */
  actions: DialogAction[];
}

const [dialog, setDialog] = createSignal<DialogState | null>(null);
function openDialog(state: DialogState) {
  setDialog(state);
}
function closeDialog() {
  setDialog(null);
}

function requestConfirm(opts: {
  title?: string;
  message: string;
  confirmText?: string;
  cancelText?: string;
  danger?: boolean;
  onConfirm: () => void | Promise<void>;
}) {
  openDialog({
    title: opts.title ?? "确认操作",
    message: opts.message,
    actions: [
      { label: opts.cancelText ?? "取消", onClick: () => {} },
      { label: opts.confirmText ?? "确定", danger: opts.danger, onClick: () => opts.onConfirm() },
    ],
  });
}

function requestPrompt(opts: {
  title?: string;
  message?: string;
  defaultValue?: string;
  placeholder?: string;
  confirmText?: string;
  onSubmit: (value: string) => void | Promise<void>;
}) {
  openDialog({
    title: opts.title ?? "输入",
    message: opts.message,
    input: { defaultValue: opts.defaultValue, placeholder: opts.placeholder },
    actions: [
      { label: "取消", onClick: () => {} },
      { label: opts.confirmText ?? "确定", onClick: (v) => opts.onSubmit(v) },
    ],
  });
}

export function useUIStore() {
  return {
    viewMode,
    thumbnailSize,
    sidebarOpen,
    sidebarTab,
    searchQuery,
    showSearch,
    infoPanelOpen,
    infoPanelFile,
    isIndexing,
    showSettings,
    showTrash,
    trashHover,
    contextMenu,
    toasts,
    dialog,
    prewarmEnabled,
    showToast,
    dismissToast,
    openDialog,
    closeDialog,
    requestConfirm,
    requestPrompt,
    setViewMode,
    setThumbnailSize,
    setSidebarOpen,
    setSidebarTab,
    setSearchQuery,
    setShowSearch,
    setInfoPanelOpen,
    setInfoPanelFile,
    setIsIndexing,
    setShowSettings,
    setShowTrash,
    setTrashHover,
    setContextMenu,
    setPrewarmEnabled,
    syncPrewarmToBackend,
  };
}

export { prewarmEnabled, setPrewarmEnabled, syncPrewarmToBackend };
