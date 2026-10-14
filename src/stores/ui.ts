import { createSignal } from "solid-js";

export type ViewMode = "grid" | "list" | "masonry";

const [viewMode, setViewMode] = createSignal<ViewMode>("grid");
const [thumbnailSize, setThumbnailSize] = createSignal(220);
const [sidebarOpen, setSidebarOpen] = createSignal(true);
const [searchQuery, setSearchQuery] = createSignal("");
const [showSearch, setShowSearch] = createSignal(false);
const [infoPanelOpen, setInfoPanelOpen] = createSignal(false);
const [infoPanelFile, setInfoPanelFile] = createSignal<string | null>(null);
const [isIndexing, setIsIndexing] = createSignal(false);
const [showSettings, setShowSettings] = createSignal(false);
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
    searchQuery,
    showSearch,
    infoPanelOpen,
    infoPanelFile,
    isIndexing,
    showSettings,
    trashHover,
    contextMenu,
    toasts,
    dialog,
    showToast,
    dismissToast,
    openDialog,
    closeDialog,
    requestConfirm,
    requestPrompt,
    setViewMode,
    setThumbnailSize,
    setSidebarOpen,
    setSearchQuery,
    setShowSearch,
    setInfoPanelOpen,
    setInfoPanelFile,
    setIsIndexing,
    setShowSettings,
    setTrashHover,
    setContextMenu,
  };
}
