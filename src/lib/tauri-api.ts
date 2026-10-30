/**
 * Tauri API abstraction layer
 * Uses Tauri invoke when running inside Tauri, falls back to mock data for dev.
 */

import { invoke as tauriInvoke } from "@tauri-apps/api/core";

// Detect if running inside Tauri
const isTauri = (): boolean => {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
};

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri()) {
    return tauriInvoke<T>(cmd, args);
  }
  return mockInvoke<T>(cmd, args);
}

// ── Mock Data ────────────────────────────────────

const mockFiles: FileInfo[] = Array.from({ length: 50 }, (_, i) => {
  const modified = Date.now() - Math.floor(Math.random() * 30 * 24 * 3600 * 1000);
  const created = modified - Math.floor(Math.random() * 120 * 24 * 3600 * 1000);
  return {
    id: i + 1,
    path: `C:\\Photos\\IMG_${String(i + 1).padStart(4, "0")}.jpg`,
    filename: `IMG_${String(i + 1).padStart(4, "0")}.jpg`,
    extension: "jpg",
    size_bytes: Math.floor(Math.random() * 10_000_000) + 500_000,
    width: Math.random() > 0.3 ? 4000 + Math.floor(Math.random() * 2000) : null,
    height: Math.random() > 0.3 ? 3000 + Math.floor(Math.random() * 1500) : null,
    rating: Math.random() > 0.6 ? Math.floor(Math.random() * 5) + 1 : null,
    color_label: null,
    notes: null,
    favorite: Math.random() > 0.7,
    mtime: new Date(modified).toISOString(),
    ctime: new Date(created).toISOString(),
    created_at: new Date(Math.max(created, modified) + 3600 * 1000).toISOString(),
    updated_at: new Date(modified).toISOString(),
    tags: [],
  };
});

const mockTags: Tag[] = [
  { id: 1, name: "风景", parent_id: null, color: "#4c6ef5", icon: null },
  { id: 2, name: "人像", parent_id: null, color: "#e64980", icon: null },
  { id: 3, name: "界面设计", parent_id: null, color: "#12b886", icon: null },
  { id: 4, name: "参考素材", parent_id: null, color: "#fab005", icon: null },
  { id: 5, name: "Logo", parent_id: null, color: "#7950f2", icon: null },
];

// ── Mock Implementation ──────────────────────────

async function mockInvoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  await new Promise((r) => setTimeout(r, 50 + Math.random() * 100));

  switch (cmd) {
    case "open_library": {
      return {
        library: {
          id: "mock-lib-1",
          name: "示例图库",
          root_path: args?.path || "C:\\Photos",
          is_network: false,
          file_count: 50,
          thumbnail_cache_size: 0,
        },
        initial_file_count: 50,
      } as unknown as T;
    }

    case "create_library": {
      const name = (args?.name as string) || "新建图库";
      return {
        library: {
          id: "mock-lib-new",
          name,
          root_path: `${args?.parentDir || "C:\\Photos"}\\${name}`,
          is_network: false,
          file_count: 0,
          thumbnail_cache_size: 0,
        },
        initial_file_count: 0,
      } as unknown as T;
    }

    case "get_libraries": {
      return [] as unknown as T;
    }

    case "get_files": {
      const offset = (args?.offset as number) || 0;
      const limit = (args?.limit as number) || 50;
      let files = mockFiles;
      if (args?.filterFavorite) files = files.filter((f) => f.favorite);
      if (args?.filterMinRating) files = files.filter((f) => (f.rating || 0) >= (args.filterMinRating as number));
      if (args?.filterColor) files = files.filter((f) => f.color_label === args.filterColor);
      const sortBy = (args?.sortBy as string) || "mtime";
      const sorted = [...files].sort((a, b) => {
        const av = sortBy === "name" ? a.filename : sortBy === "size" ? a.size_bytes : new Date(a.mtime).getTime();
        const bv = sortBy === "name" ? b.filename : sortBy === "size" ? b.size_bytes : new Date(b.mtime).getTime();
        return av < bv ? 1 : av > bv ? -1 : 0;
      });
      return {
        files: sorted.slice(offset, offset + limit),
        total: sorted.length,
        offset,
        limit,
      } as unknown as T;
    }

    case "get_file_info": {
      const path = args?.file_path as string;
      const file = mockFiles.find((f) => f.path === path) || mockFiles[0];
      return file as unknown as T;
    }

    case "update_file_rating":
    case "update_file_notes":
    case "delete_file":
    case "set_file_favorite":
    case "set_file_color":
      return null as unknown as T;

    case "get_folder_tree":
      return {
        name: "Photos",
        path: "C:\\Photos",
        file_count: 12,
        children: [
          { name: "2024", path: "C:\\Photos\\2024", file_count: 8, children: [] },
          { name: "2025", path: "C:\\Photos\\2025", file_count: 4, children: [] },
        ],
      } as unknown as T;

    case "get_view_counts":
      return {
        all: 50,
        favorite: 15,
        rated: 20,
        trash: 0,
        colors: { red: 3, blue: 5 },
      } as unknown as T;

    case "get_thumbnail_path":
    case "get_preview_path": {
      return { status: "ready", path: "/placeholder.svg" } as unknown as T;
    }

    case "get_thumbnail_progress": {
      return { total: 0, done: 0, running: false } as unknown as T;
    }

    case "get_warm_progress":
      return [] as unknown as T;

    case "get_prewarm_enabled":
      return true as unknown as T;

    case "set_prewarm_enabled":
      return undefined as unknown as T;

    case "build_missing_thumbnails":
      return 0 as unknown as T;

    case "thumbs_interaction":
      return null as unknown as T;

    case "get_gpu_status":
      return {
        mode: "auto",
        software_rendering: false,
        crashed_last_run: false,
      } as unknown as T;

    case "set_gpu_mode":
      return {
        mode: (args?.mode as string) || "auto",
        software_rendering: (args?.mode as string) === "software",
        crashed_last_run: false,
      } as unknown as T;

    case "ui_ready":
      return undefined as unknown as T;

    case "get_all_tags": {
      return mockTags as unknown as T;
    }

    case "create_tag": {
      return {
        id: mockTags.length + 1,
        name: args?.name as string,
        parent_id: args?.parent_id || null,
        color: args?.color || null,
        icon: null,
      } as unknown as T;
    }

    case "add_tag_to_file":
    case "remove_tag_from_file":
    case "batch_add_tags":
      return null as unknown as T;

    case "search_files": {
      return {
        files: mockFiles.slice(0, 20),
        total: 20,
        query_time_ms: 5,
      } as unknown as T;
    }

    case "find_duplicates": {
      return [] as unknown as T;
    }

    case "get_nas_status": {
      return { status: "local", latency_ms: 0 } as unknown as T;
    }

    case "index_library": {
      return { total: 50, processed: 50, stage: "complete" } as unknown as T;
    }

    case "reorder_libraries":
      return null as unknown as T;

    case "detect_library_kind":
      return "unknown" as unknown as T;

    case "probe_eaglepack":
      return {
        layout: "pack",
        item_count: 0,
        folder_count: 0,
        entry_count: 0,
        unpacked_bytes: 0,
      } as unknown as T;

    case "cancel_eaglepack_import":
      return null as unknown as T;

    case "import_eagle_library":
    case "import_pixcall_library":
    case "import_eaglepack":
      return {
        kind: "eagle",
        detected: true,
        total_items: 0,
        imported_files: 0,
        created_tags: 0,
        errors: [],
        library: {
          id: "mock-lib-2",
          name: "导入的库",
          root_path: "C:\\Imported",
          is_network: false,
          file_count: 0,
          thumbnail_cache_size: 0,
        },
      } as unknown as T;

    case "get_deleted_files":
      return [] as unknown as T;

    case "get_trash":
      return { entries: [], sources: [], unavailable: [] } as unknown as T;

    case "get_trash_count":
      return 0 as unknown as T;

    case "trash_files":
      return 0 as unknown as T;

    case "restore_file":
    case "purge_file":
      return null as unknown as T;

    case "empty_trash":
    case "empty_trash_all":
      return { removed: 0, failed: [] } as unknown as T;

    default:
      console.warn(`Mock: unknown command "${cmd}"`);
      return null as unknown as T;
  }
}

// ── Exported Types ───────────────────────────────

export interface FileInfo {
  id: number;
  path: string;
  filename: string;
  extension: string;
  size_bytes: number;
  width: number | null;
  height: number | null;
  rating: number | null;
  color_label: string | null;
  notes: string | null;
  /** 收藏标记（Pixcall 式 ♥） */
  favorite: boolean;
  /** 文件系统修改时间 */
  mtime: string;
  /** 文件系统创建时间 */
  ctime: string;
  /** 导入图库的时间 */
  created_at: string;
  /** 图库记录最后更新时间 */
  updated_at: string;
  tags: Tag[];
}

export interface Tag {
  id: number;
  name: string;
  parent_id: number | null;
  color: string | null;
  icon: string | null;
}

export interface LibraryInfo {
  id: string;
  name: string;
  root_path: string;
  is_network: boolean;
  file_count: number;
  thumbnail_cache_size: number;
}

/** 侧边栏「文件夹」树的一个节点。 */
export interface FolderNode {
  name: string;
  /** 完整路径（系统原生分隔符，与 FileInfo.path 一致）。 */
  path: string;
  /** 直接放在该文件夹里的图片数（不含子文件夹）。 */
  file_count: number;
  children: FolderNode[];
}

/** 颜色标记色板（与后端 COLOR_LABELS 一致）。 */
export const COLOR_LABELS: { name: string; hex: string }[] = [
  { name: "red", hex: "#e03131" },
  { name: "orange", hex: "#f76707" },
  { name: "yellow", hex: "#f59f00" },
  { name: "green", hex: "#2f9e44" },
  { name: "blue", hex: "#1971c2" },
  { name: "purple", hex: "#9c36b5" },
  { name: "gray", hex: "#868e96" },
];

/** 智能视图 / 颜色标记的计数徽标。 */
export interface ViewCounts {
  all: number;
  favorite: number;
  rated: number;
  trash: number;
  colors: Record<string, number>;
}

/**
 * 回收站条目：文件信息 + 来源库。
 *
 * 回收站是**逻辑库**，不搬文件、没有自己的目录。条目带着来源库，
 * 恢复 / 彻底删除时按 `library_id` 回原库执行，不存在跨库写入。
 */
export interface TrashEntry extends FileInfo {
  library_id: string;
  library_name: string;
}

/** 回收站里某个来源库的汇总，用于顶部的来源筛选条。 */
export interface TrashSource {
  library_id: string;
  library_name: string;
  count: number;
}

export interface TrashPayload {
  entries: TrashEntry[];
  sources: TrashSource[];
  /** 读不出来的库名（NAS 离线、盘不在）。UI 必须标注，不能假装它没有回收站。 */
  unavailable: string[];
}

/** 清空回收站的结果：删不掉的条目会把记录留下，方便重试。 */
export interface EmptyTrashResult {
  removed: number;
  failed: string[];
}

/** 当前浏览的「视图」——侧边栏点谁就是谁，网格按它加载。 */
export type ViewKind = "all" | "recent" | "favorite" | "rated" | "color" | "folder" | "tag" | "trash";

export interface ViewState {
  kind: ViewKind;
  /** kind = "folder" 时的文件夹完整路径。 */
  folder?: string;
  /** kind = "color" 时的颜色名。 */
  color?: string;
  /** kind = "tag" 时的标签 id。 */
  tagId?: number;
}

export interface PaginatedFiles {
  files: FileInfo[];
  total: number;
  offset: number;
  limit: number;
}

/** 缩略图查询结果。
 *  - `ready`：path 是缩略图文件
 *  - `fallback`：这张图生成不出缩略图（RAW/PSD 等），path 是原图
 *  - `pending`：已排进后台队列，稍后会收到 `thumb-ready` 事件
 */
export interface ThumbnailResult {
  status: "ready" | "fallback" | "pending";
  path: string;
}

/** 批量缩略图查询的单条结果（语义同 ThumbnailResult，外加占位图）。 */
export interface ThumbBatchItem {
  path: string;
  status: "ready" | "fallback" | "pending";
  /** ready / fallback 时的图片地址。 */
  thumb: string;
  /** 已缓存的模糊占位图（32px data URL）；没有则为空串。 */
  placeholder: string;
}

/** 后台缩略图任务的进度。 */
export interface ThumbProgress {
  total: number;
  done: number;
  running: boolean;
}

/**
 * 某个图库的「缩略图预热」进度。
 *
 * 打开 / 导入 / 恢复图库时会自动把还缺缩略图的文件排进后台队列（默认启用），
 * 侧边栏据此在图库名旁边画一个圆形进度环。
 */
export interface WarmProgress {
  library_id: string;
  /** 本轮需要补齐的图片总数。 */
  total: number;
  /** 已处理数量。 */
  done: number;
  /** 是否仍在处理。 */
  running: boolean;
}

/** `thumb-ready` 事件的载荷元素。`thumb` 为 null 表示生成失败、应退回原图。 */
export interface ThumbReadyItem {
  path: string;
  thumb: string | null;
}

/** `thumb-placeholder` 事件的载荷元素：一张图的模糊占位图（32px data URL）。 */
export interface ThumbPlaceholderItem {
  path: string;
  data: string;
}

export interface NasStatus {
  status: "local" | "online" | "degraded" | "offline";
  latency_ms: number;
}

export interface ImportSummary {
  kind: string;
  detected: boolean;
  total_items: number;
  imported_files: number;
  created_tags: number;
  errors: string[];
  /** 仅 eaglepack：实际写盘的字节数 */
  unpacked_bytes?: number;
  /** 仅 eaglepack：跳过/失败的总数 */
  skipped?: number;
  /** 仅 eaglepack：按 Eagle 文件夹层级重组落盘的文件数 */
  reorganized?: number;
  /** 仅 eaglepack：因同名被自动加上 `(2)` 序号的个数 */
  renamed?: number;
  library: LibraryInfo;
}

/** `probe_eaglepack` 的返回：开包前的统计，只读中央目录，不解压。 */
export interface PackProbe {
  /** `pack`（pack.json + <ID>.info/）或 `library`（metadata.json + images/） */
  layout: string;
  item_count: number;
  folder_count: number;
  entry_count: number;
  /** 解压后总大小（字节） */
  unpacked_bytes: number;
}

/** `eaglepack-progress` 事件的载荷。 */
export interface PackProgress {
  /** `unpack` 解压中 / `parse` 解析元数据 / `organize` 按文件夹重组落盘 */
  phase: "unpack" | "parse" | "organize";
  done: number;
  total: number;
  current: string;
}

export interface SearchResult {
  files: FileInfo[];
  total: number;
  query_time_ms: number;
}

export interface DuplicateGroup {
  hash: string;
  file_count: number;
  wasted_bytes: number;
  match_type: "exact" | "perceptual";
  files: FileInfo[];
}

/** 渲染方式。`auto` = 跟着崩溃哨兵走；`hardware` = 强制尝试 GPU（仍保留崩溃保护）。 */
export type GpuMode = "auto" | "hardware" | "software";

export interface GpuStatus {
  mode: GpuMode;
  /** 当前生效的是不是软件渲染。 */
  software_rendering: boolean;
  /** 是否有过「启动没走到界面就绪」的记录 —— 自动降级的原因。 */
  crashed_last_run: boolean;
}

// ── 浏览器扩展桥接 ──────────────────────────────

export interface BridgeLibraryRef {
  id: string;
  name: string;
  root_path: string;
}

export interface BridgeRecentItem {
  path: string;
  filename: string;
  size: number;
  page: string;
  at: string;
  library_id: string;
}

export interface BridgeStatus {
  enabled: boolean;
  port: number;
  /** 是否真的在监听（端口被占用 / 应用刚启动时为 false） */
  listening: boolean;
  bound_port: number;
  /** 扩展里要填的访问令牌 */
  token: string;
  last_error: string;
  /** 累计请求数 / 累计入库文件数 */
  requests: number;
  ingested: number;
  recent: BridgeRecentItem[];
  library: BridgeLibraryRef | null;
}

// ── API Functions ────────────────────────────────

/** 交互上报的最小间隔（毫秒）。后端静默窗口约 460ms，120ms 足够把「连续滚动」
 *  压成每秒 8 次左右的 IPC，又不至于让后端误以为用户已经停手。 */
const INTERACTION_PING_MS = 120;
let lastInteractionPing = 0;

export const api = {
  // Library
  openLibrary: (path: string) =>
    invoke<{ library: LibraryInfo; initial_file_count: number }>("open_library", { path }),

  createLibrary: (parentDir: string, name: string) =>
    invoke<{ library: LibraryInfo; initial_file_count: number }>("create_library", { parentDir, name }),

  closeLibrary: (id: string) =>
    invoke<void>("close_library", { id }),

  renameLibrary: (id: string, newName: string) =>
    invoke<void>("rename_library", { id, newName }),

  /**
   * 删除库。`deleteFiles = false` 只从列表移除（磁盘原样保留）。
   *
   * `deleteFiles = true` 会 `remove_dir_all` 整个库根目录 —— 不进回收站。
   * 后端要求 `confirmName` 与库名逐字相同才肯执行，多一道防止误删的闸。
   */
  deleteLibrary: (id: string, deleteFiles: boolean, confirmName?: string) =>
    invoke<void>("delete_library", { id, deleteFiles, confirmName }),

  getLibraries: () =>
    invoke<LibraryInfo[]>("get_libraries"),

  indexLibrary: (libraryId: string) =>
    invoke<{ total: number; processed: number; stage: string }>("index_library", { libraryId }),

  getNasStatus: (libraryId: string) =>
    invoke<NasStatus>("get_nas_status", { libraryId }),

  // Files
  getFiles: (
    libraryId: string,
    offset: number,
    limit: number,
    sortBy = "mtime",
    sortOrder = "DESC",
    filterExt?: string,
    filterTag?: number,
    filterFolder?: string,
    filterFavorite?: boolean,
    filterColor?: string,
    filterMinRating?: number,
    filterRecentDays?: number
  ) =>
    invoke<PaginatedFiles>("get_files", {
      libraryId,
      offset,
      limit,
      sortBy,
      sortOrder,
      filterExt,
      filterTag,
      filterFolder,
      filterFavorite,
      filterColor,
      filterMinRating,
      filterRecentDays,
    }),

  getFileInfo: (libraryId: string, filePath: string) =>
    invoke<FileInfo>("get_file_info", { libraryId, filePath }),

  updateFileRating: (libraryId: string, filePath: string, rating: number) =>
    invoke<void>("update_file_rating", { libraryId, filePath, rating }),

  updateFileNotes: (libraryId: string, filePath: string, notes: string) =>
    invoke<void>("update_file_notes", { libraryId, filePath, notes }),

  /** 收藏 / 取消收藏。 */
  setFileFavorite: (libraryId: string, filePath: string, favorite: boolean) =>
    invoke<void>("set_file_favorite", { libraryId, filePath, favorite }),

  /** 设置 / 清除颜色标记（传 null 清除）。 */
  setFileColor: (libraryId: string, filePath: string, color: string | null) =>
    invoke<void>("set_file_color", { libraryId, filePath, color }),

  deleteFile: (libraryId: string, filePath: string) =>
    invoke<void>("delete_file", { libraryId, filePath }),

  trashFiles: (libraryId: string, filePaths: string[]) =>
    invoke<number>("trash_files", { libraryId, filePaths }),

  getDeletedFiles: (libraryId: string) =>
    invoke<FileInfo[]>("get_deleted_files", { libraryId }),

  /** 跨库聚合回收站：回收站是「逻辑库」，一次拿全部来源库的条目。 */
  getTrash: () => invoke<TrashPayload>("get_trash"),

  /** 跨库回收站总条目数（侧栏徽标用，只 COUNT 不取行）。 */
  getTrashCount: () => invoke<number>("get_trash_count"),

  restoreFile: (libraryId: string, filePath: string) =>
    invoke<void>("restore_file", { libraryId, filePath }),

  /** `alsoDeleteDisk` 为真时连磁盘文件一起删（后端会校验路径在库根内）。 */
  purgeFile: (libraryId: string, filePath: string, alsoDeleteDisk = false) =>
    invoke<void>("purge_file", { libraryId, filePath, alsoDeleteDisk }),

  emptyTrash: (libraryId: string, alsoDeleteDisk = false) =>
    invoke<EmptyTrashResult>("empty_trash", { libraryId, alsoDeleteDisk }),

  /** 清空所有库的回收站。 */
  emptyTrashAll: (alsoDeleteDisk = false) =>
    invoke<EmptyTrashResult>("empty_trash_all", { alsoDeleteDisk }),

  getThumbnailPath: (libraryId: string, filePath: string) =>
    invoke<ThumbnailResult>("get_thumbnail_path", { libraryId, filePath }),

  /**
   * 批量取缩略图（一次 IPC 一整屏）。
   *
   * `window` 是前端声明的「当前视口路径集合」：除了给这些图插队，还会把窗口
   * 之外的过期 urgent 任务降级回整库队列。虚拟化网格滚动时由父组件整屏提交。
   */
  getThumbnails: (libraryId: string, paths: string[], window?: string[]) =>
    invoke<ThumbBatchItem[]>("get_thumbnails", {
      libraryId,
      paths,
      window: window ?? null,
    }),

  /** 右侧预览栏用的 1600px 预览图（按需生成，结果走 `preview-ready` 事件）。 */
  getPreviewPath: (libraryId: string, filePath: string) =>
    invoke<ThumbnailResult>("get_preview_path", { libraryId, filePath }),

  /** 兜底：直接取缩略图的 data URL（绕开 asset 协议，用于加载失败时）。 */
  readThumbDataUrl: (libraryId: string, filePath: string) =>
    invoke<string | null>("read_thumb_data_url", { libraryId, filePath }),

  /** 把前端诊断信息写进后端日志（render-debug.log）。 */
  diagLog: (tag: string, msg: string) =>
    invoke<void>("diag_log", { tag, msg }).catch(() => {}),

  /** 通知后端「界面已经渲染出来」——用于注销 GPU 崩溃哨兵。 */
  uiReady: (diag?: string) =>
    invoke<void>("ui_ready", { diag }),

  getThumbnailProgress: () =>
    invoke<ThumbProgress>("get_thumbnail_progress"),

  /** 所有图库的缩略图预热进度（侧边栏库名旁的圆环）。 */
  getWarmProgress: () =>
    invoke<WarmProgress[]>("get_warm_progress"),

  /** 读取「打开图库时后台补齐缩略图」开关。 */
  getPrewarmEnabled: () =>
    invoke<boolean>("get_prewarm_enabled"),

  /** 设置「打开图库时后台补齐缩略图」。默认开启。 */
  setPrewarmEnabled: (enabled: boolean) =>
    invoke<void>("set_prewarm_enabled", { enabled }),

  /* 手动补齐缺失缩略图（不受预热开关影响）。返回本轮排入队列的数量。 */
  buildMissingThumbnails: (libraryId: string) =>
    invoke<number>("build_missing_thumbnails", { libraryId }),

  /**
   * 上报「用户正在滚动 / 拖拽」。
   *
   * 后端收到后会在约 0.5 秒的静默窗口内**暂停整库预热队列**，只继续做用户
   * 当前视口（urgent）的任务，把 CPU 让给界面渲染；窗口到期自动恢复满速，
   * 所以不需要上报「交互结束」。
   *
   * **限流在函数内部**（约 120ms 一次、fire-and-forget 不 await），调用方可以
   * 直接在滚动回调里无脑调。
   */
  notifyInteraction: (): void => {
    const now = Date.now();
    if (now - lastInteractionPing < INTERACTION_PING_MS) return;
    lastInteractionPing = now;
    void invoke<void>("thumbs_interaction").catch(() => {});
  },

  /** 当前渲染方式、是否软件渲染、有没有被自动降级过。 */
  getGpuStatus: () => invoke<GpuStatus>("get_gpu_status"),

  /** 切换渲染方式。**重启应用后生效**（WebView2 启动参数只在建窗时能传）。 */
  setGpuMode: (mode: GpuMode) => invoke<GpuStatus>("set_gpu_mode", { mode }),

  openInExplorer: (filePath: string) =>
    invoke<void>("open_in_explorer", { filePath }),

  openFile: (filePath: string) =>
    invoke<void>("open_file", { filePath }),

  copyFilesToClipboard: (filePaths: string[]) =>
    invoke<number>("copy_files_to_clipboard", { filePaths }),

  // Tags
  getAllTags: (libraryId: string) =>
    invoke<Tag[]>("get_all_tags", { libraryId }),

  createTag: (libraryId: string, name: string, parentId?: number, color?: string) =>
    invoke<Tag>("create_tag", { libraryId, name, parentId, color }),

  addTagToFile: (libraryId: string, filePath: string, tagId: number) =>
    invoke<void>("add_tag_to_file", { libraryId, filePath, tagId }),

  removeTagFromFile: (libraryId: string, filePath: string, tagId: number) =>
    invoke<void>("remove_tag_from_file", { libraryId, filePath, tagId }),

  batchAddTags: (libraryId: string, filePaths: string[], tagIds: number[]) =>
    invoke<string[]>("batch_add_tags", { libraryId, filePaths, tagIds }),

  // Search
  searchFiles: (libraryId: string, params: { text?: string }) =>
    invoke<SearchResult>("search_files", { libraryId, text: params.text || null }),

  // 资源库视图（Pixcall 式侧边栏）
  /** 库内文件夹树（侧边栏「文件夹」区）。 */
  getFolderTree: (libraryId: string) =>
    invoke<FolderNode>("get_folder_tree", { libraryId }),

  /** 智能视图 / 颜色标记的计数徽标。 */
  getViewCounts: (libraryId: string) =>
    invoke<ViewCounts>("get_view_counts", { libraryId }),

  // Persist the display order of the opened libraries
  reorderLibraries: (ids: string[]) =>
    invoke<void>("reorder_libraries", { ids }),

  // Third-party library import
  detectLibraryKind: (path: string) =>
    invoke<string>("detect_library_kind", { path }),

  importEagleLibrary: (sourcePath: string) =>
    invoke<ImportSummary>("import_eagle_library", { sourcePath }),

  importPixcallLibrary: (sourcePath: string) =>
    invoke<ImportSummary>("import_pixcall_library", { sourcePath }),

  /** 只读 zip 中央目录，出统计（不解压）。 */
  probeEaglepack: (packPath: string) =>
    invoke<PackProbe>("probe_eaglepack", { packPath }),

  /** 流式解压到 destDir，然后把 destDir 当作新图库打开。 */
  importEaglepack: (packPath: string, destDir: string) =>
    invoke<ImportSummary>("import_eaglepack", { packPath, destDir }),

  cancelEaglepackImport: () =>
    invoke<void>("cancel_eaglepack_import"),

  // 浏览器扩展桥接
  getBridgeStatus: () => invoke<BridgeStatus>("get_bridge_status"),

  setBridgeEnabled: (enabled: boolean) =>
    invoke<BridgeStatus>("set_bridge_enabled", { enabled }),

  setBridgePort: (port: number) =>
    invoke<BridgeStatus>("set_bridge_port", { port }),

  regenerateBridgeToken: () =>
    invoke<BridgeStatus>("regenerate_bridge_token"),

  /** 把内置扩展写进用户选的目录，返回文件数 */
  exportExtension: (destDir: string) =>
    invoke<number>("export_extension", { destDir }),

  revealPath: (path: string) => invoke<void>("reveal_path", { path }),

  // Restore
  restoreLibraries: () =>
    invoke<{ libraries: LibraryInfo[]; failed_paths: string[] }>("restore_libraries"),

  searchByColor: (libraryId: string, colorHex: string) =>
    invoke<FileInfo[]>("search_by_color", { libraryId, colorHex }),

  findDuplicates: (libraryId: string) =>
    invoke<DuplicateGroup[]>("find_duplicates", { libraryId }),
};