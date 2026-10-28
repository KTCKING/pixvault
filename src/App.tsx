import { createSignal, createEffect, untrack, For, onMount, onCleanup, Show } from "solid-js";
import { useLibraryStore, activeLibraryId } from "@/stores/library";
import { useFilesStore } from "@/stores/files";
import { useTagsStore } from "@/stores/tags";
import { useUIStore } from "@/stores/ui";
import { useTheme } from "@/stores/theme";
import { Sidebar } from "@/components/Sidebar";
import { ThumbnailGrid } from "@/components/ThumbnailGrid";
import { StatusBar } from "@/components/StatusBar";
import { WelcomeScreen } from "@/components/WelcomeScreen";
import { InfoPanel } from "@/components/InfoPanel";
import { SearchBar } from "@/components/SearchBar";
import {
  IconChevronDown,
  IconChevronLeft,
  IconChevronRight,
  IconFolder,
  IconGlobe,
  IconLibrary,
  IconLoader,
  IconMoon,
  IconPanelRight,
  IconSettings,
  IconSun,
  StatusDot,
} from "@/components/icons";
import { HeroLogo } from "@/components/icons/hero";
import { ImageViewer } from "@/components/ImageViewer";
import { IndexingDialog } from "@/components/IndexingDialog";
import { SettingsModal } from "@/components/SettingsModal";
import { ContextMenu } from "@/components/ContextMenu";
import { ToastHost } from "@/components/Toast";
import { DialogHost } from "@/components/Dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import { api } from "@/lib/tauri-api";
import { useViewerStore } from "@/stores/viewer";

export default function App() {
  const library = useLibraryStore();
  const files = useFilesStore();
  const tags = useTagsStore();
  const ui = useUIStore();
  const theme = useTheme();

  const viewer = useViewerStore();

  const [dragOver, setDragOver] = createSignal(false);
  /** 顶部「当前库」切换下拉是否展开。 */
  const [libMenuOpen, setLibMenuOpen] = createSignal(false);

  /**
   * 切换 / 打开库后重置视图并重载全部库相关数据（侧边栏计数、文件夹树、标签、网格）。
   *
   * 这个 effect 只能依赖「当前库 id」，绝不能依赖 `currentView`：
   * `setView` 的同步段会读到 `currentView()`，若在 effect 里裸调用，点击侧边栏
   * 智能视图会让本 effect 重跑、立刻把视图重置回「所有图片」——表现为点了没反应。
   * 因此内部全部包进 untrack，并用 lastLibraryId 做「真的换库才重载」的守卫。
   */
  let lastLibraryId: string | null = null;
  createEffect(() => {
    const id = activeLibraryId();
    if (!id) {
      lastLibraryId = null;
      return;
    }
    if (id === lastLibraryId) return;
    lastLibraryId = id;
    untrack(() => {
      files.setCurrentSort({ by: "mtime", order: "DESC" });
      void files.setView({ kind: "all" });
      void tags.loadTags();
      void files.loadFolderTree();
      void files.loadViewCounts();
    });
  });

  /** Toggle the right-hand info / preview panel. When re-opening it prefers
   *  the file that was shown last, then the first selected file, then the first
   *  file of the list; with nothing to show the panel still opens and reports
   *  the library's file count (like Pixcall's right column). */
  const toggleInfoPanel = () => {
    if (ui.infoPanelOpen()) {
      ui.setInfoPanelOpen(false);
      return;
    }
    const current = ui.infoPanelFile();
    const list = files.files();
    const target =
      (current && list.some((f) => f.path === current) ? current : null) ??
      [...files.selectedFiles()][0] ??
      list[0]?.path ??
      null;
    ui.setInfoPanelFile(target);
    ui.setInfoPanelOpen(true);
  };

  onMount(async () => {
    // 界面已经渲染出来了：通知后端注销 GPU 崩溃哨兵。如果这一步没跑到
    // （例如 WebView2 的 GPU 进程起不来导致整个窗口白屏），下次启动会自动
    // 降级到软件渲染。
    void api.uiReady();

    // 先把「打开图库时是否自动补齐缩略图」的本地偏好推给后端，再恢复图库 ——
    // 顺序反过来的话，用户明明关掉了预热，重启仍会被恢复流程先跑一整轮。
    await ui.syncPrewarmToBackend();

    // 浏览器扩展存进来一个文件：刷一下列表，让用户当场看到它。子文件夹可能
    // 是新出现的（按站点建目录），所以文件夹树也要重读。
    const unBridge = await listen<{ filename: string; library_id: string }>(
      "bridge-ingested",
      (e) => {
        if (e.payload.library_id !== activeLibraryId()) return;
        ui.showToast(`已从网页存入 ${e.payload.filename}`, "success");
        void files.loadFiles(0);
        void files.loadFolderTree();
        void files.loadViewCounts();
      },
    );
    onCleanup(unBridge);

    // Watchdog: force-clear loading after 3s even if restore hangs
    const watchdog = setTimeout(() => {
      library.setIsRestoring(false);
    }, 3000);

    // Restore previously opened libraries
    try {
      const saved = await api.restoreLibraries();
      library.setRestoredLibraries(saved.libraries);

      if (saved.libraries.length === 1) {
        // Single library: auto-open it
        library.setLibraries(saved.libraries);
        library.setActiveLibraryId(saved.libraries[0].id);
        files.loadFiles(0);
        tags.loadTags();
      } else if (saved.libraries.length > 1) {
        // Multiple libraries: show picker
        library.setLibraries(saved.libraries);
        library.setShowLibraryPicker(true);
      }
      // 0 libraries: stays on welcome screen (no action needed)
    } catch (e) {
      console.error("Failed to restore libraries:", e);
    }
    library.setIsRestoring(false);
    clearTimeout(watchdog);

    // window 上的 keydown 会有多个监听器（这里 + ImageViewer + ContextMenu）。同一次按键
    // 里预览器先处理 Esc 并把自己关掉，等这里执行时 `viewer.viewerOpen()` 已经是 false ——
    // 结果按一次 Esc 会「关预览 + 连带关掉右侧信息栏和搜索框」，用户得按两次。
    // 捕获阶段的监听器一定早于所有冒泡监听器（与注册顺序无关），用它把「事件开始派发时」
    // 的状态拍下来：只要当时有浮层开着（预览 / 右键菜单 / 确认弹窗），这次按键就归浮层，
    // App 这一层不碰背景状态（否则剪贴板菜单按下 Esc 会顺手清掉选中项）。
    let overlayWasOpen = false;
    const snapshotOverlayState = () => {
      overlayWasOpen =
        viewer.viewerOpen() || ui.contextMenu() !== null || ui.dialog() !== null;
    };
    window.addEventListener("keydown", snapshotOverlayState, true);

    const handleKeyDown = (e: KeyboardEvent) => {
      // Don't handle app shortcuts when an overlay is open (it handles its own keys)
      if (overlayWasOpen) return;
      if (e.ctrlKey && e.key === "f") {
        e.preventDefault();
        ui.setShowSearch(true);
      }
      if (e.key === "Escape") {
        files.clearSelection();
        ui.setShowSearch(false);
        ui.setInfoPanelOpen(false);
      }
      if (e.key === "F5") {
        e.preventDefault();
        files.loadFiles(0);
      }
      if (e.key === "Delete" && files.selectedFiles().size > 0) {
        files.trashFiles();
        files.clearSelection();
      }
    };

    window.addEventListener("keydown", handleKeyDown);

    // Only folders should open a library. Image files can land here when a
    // card is dragged back over the window during an OS drag — ignore those.
    const IMAGE_EXT = /\.(jpe?g|png|gif|webp|bmp|tiff?|avif|svg|psd|raw|cr2|nef|arw|dng|heic|ico)$/i;

    const unlisten = await getCurrentWindow().onDragDropEvent((event) => {
      if (event.payload.type === "over") {
        setDragOver(true);
      } else if (event.payload.type === "leave") {
        setDragOver(false);
      } else if (event.payload.type === "drop") {
        setDragOver(false);
        const paths = event.payload.paths;
        if (paths.length > 0 && !IMAGE_EXT.test(paths[0])) {
          // 数据重载交给 activeLibraryId 的 effect，这里只负责开库。
          library.openLibrary(paths[0]);
        }
      }
    });

    onCleanup(() => {
      window.removeEventListener("keydown", snapshotOverlayState, true);
      window.removeEventListener("keydown", handleKeyDown);
      unlisten();
    });
  });

  const borderColor = "var(--border-color)";

  return (
    <div class="flex flex-col h-full" style={{ background: "var(--bg-primary)", "background-image": "var(--canvas-glow)", color: "var(--text-primary)" }}>
      {/* Title Bar */}
      <header class="app-header flex items-center gap-3 px-4 py-2">
        <div class="flex items-center gap-2">
          <HeroLogo size={22} style={{ color: "var(--text-primary)" }} />
          <span class="text-sm font-semibold" style={{ color: "var(--text-primary)" }}>PixVault</span>
        </div>

        {/* 当前库切换（Pixcall 顶部的库切换器） */}
        <Show when={library.activeLibrary()}>
          <div class="relative">
            <button
              class="flex items-center gap-1.5 px-2 py-1 rounded text-xs btn-ghost"
              onClick={() => setLibMenuOpen(!libMenuOpen())}
              title="切换资源库"
            >
              <span style={{ color: "var(--text-muted)" }}>当前库</span>
              <span class="font-medium max-w-[10rem] truncate" style={{ color: "var(--text-primary)" }}>
                {library.activeLibrary()?.name}
              </span>
              <span style={{ color: "var(--text-muted)" }}>
                <IconChevronDown size={12} />
              </span>
            </button>
            <Show when={libMenuOpen()}>
              <>
                {/* 透明遮罩：点空白处关闭下拉 */}
                <div class="fixed inset-0 z-40" onClick={() => setLibMenuOpen(false)} />
                <div class="glass-modal absolute top-full left-0 mt-1 w-64 rounded-lg z-50 py-1 animate-scale-in">
                  <For each={library.libraries()}>
                    {(lib) => (
                      <button
                        class="w-full text-left px-3 py-2 text-xs transition-colors flex items-center gap-2 hover:bg-[var(--bg-hover)]"
                        style={{ color: lib.id === library.activeLibraryId() ? "var(--accent)" : "var(--text-primary)" }}
                        onClick={() => {
                          library.setActiveLibraryId(lib.id);
                          setLibMenuOpen(false);
                        }}
                      >
                        <span class="flex-shrink-0" style={{ color: "var(--text-muted)" }}>
                          {lib.is_network ? <IconGlobe size={14} /> : <IconLibrary size={14} />}
                        </span>
                        <span class="truncate flex-1">{lib.name}</span>
                        <span class="text-2xs tabular-nums flex-shrink-0" style={{ color: "var(--text-muted)" }}>
                          {lib.file_count > 0 ? lib.file_count.toLocaleString() : ""}
                        </span>
                      </button>
                    )}
                  </For>
                </div>
              </>
            </Show>
          </div>
          <Show when={library.nasStatus()}>
            <span class="text-xs ml-1 flex items-center">
              <StatusDot tone={library.nasStatus()!.status} />
            </span>
          </Show>
        </Show>

        <div class="flex-1" />

        <SearchBar />

        {/* Theme Toggle */}
        <button
          class="btn-ghost text-xs px-2 flex"
          onClick={theme.toggle}
          title={theme.theme() === "light" ? "切换到深色模式" : "切换到浅色模式"}
        >
          {theme.theme() === "light" ? <IconMoon size={15} /> : <IconSun size={15} />}
        </button>

        <button
          class="btn-ghost text-xs flex"
          onClick={() => ui.setSidebarOpen(!ui.sidebarOpen())}
          title={ui.sidebarOpen() ? "隐藏侧边栏" : "显示侧边栏"}
        >
          {ui.sidebarOpen() ? <IconChevronLeft size={15} /> : <IconChevronRight size={15} />}
        </button>

        <button
          class="btn-ghost text-xs px-2 flex"
          style={{ color: ui.infoPanelOpen() ? "var(--accent)" : "var(--text-muted)" }}
          onClick={toggleInfoPanel}
          title={ui.infoPanelOpen() ? "隐藏信息面板" : "显示信息面板"}
        >
          <IconPanelRight size={15} />
        </button>

        <button
          class="btn-ghost text-xs px-2 flex"
          onClick={() => ui.setShowSettings(true)}
          title="设置"
        >
          <IconSettings size={15} />
        </button>
      </header>

      {/* Main Content — the right-hand info panel is a normal flex sibling so
          it *occupies* space: the grid area narrows and the auto-fill column
          count drops from 6 to 5 (instead of overlaying the last column). */}
      <div class="relative flex flex-1 overflow-hidden">
        <Show when={!library.isRestoring()} fallback={
          <div class="flex-1 flex items-center justify-center" style={{ background: "var(--bg-primary)" }}>
            <div class="text-center">
              <div class="mb-3 flex justify-center" style={{ color: "var(--text-muted)" }}>
                <IconLoader size={30} class="animate-spin" />
              </div>
              <p class="text-sm" style={{ color: "var(--text-muted)" }}>正在加载...</p>
            </div>
          </div>
        }>
          <Show when={library.activeLibrary()} fallback={<WelcomeScreen onOpen={library.openLibrary} isIndexing={library.isIndexing()} />}>
          <Show when={ui.sidebarOpen()}>
            <Sidebar />
          </Show>

          <div class="flex-1 flex flex-col overflow-hidden">
            <ThumbnailGrid />
          </div>

          <Show when={ui.infoPanelOpen()}>
            <InfoPanel
              filePath={ui.infoPanelFile()}
              onClose={() => {
                ui.setInfoPanelOpen(false);
                ui.setInfoPanelFile(null);
              }}
            />
          </Show>
        </Show>
        </Show>
      </div>

      {/* Status Bar */}
      <StatusBar />

      {/* Drag Overlay */}
      <Show when={dragOver()}>
        <div class="drop-zone">
          <div class="text-center">
            <div class="mb-3 flex justify-center" style={{ color: "var(--text-primary)" }}>
              <IconFolder size={44} />
            </div>
            <p class="text-lg font-medium" style={{ color: "var(--text-primary)" }}>拖放文件夹以打开图片库</p>
            <p class="text-sm mt-1" style={{ color: "var(--text-muted)" }}>支持本地文件夹和网络路径 (SMB)</p>
          </div>
        </div>
      </Show>

      {/* Image Viewer */}
      <ImageViewer />

      {/* Global context menu */}
      <ContextMenu />

      {/* Transient toasts (copy / trash feedback) */}
      <ToastHost />

      {/* In-app confirm / prompt dialog (window.confirm is unreliable here) */}
      <DialogHost />

      {/* Settings */}
      <Show when={ui.showSettings()}>
        <SettingsModal
          onClose={() => ui.setShowSettings(false)}
          onImported={() => {
            files.loadFiles(0);
            tags.loadTags();
          }}
        />
      </Show>

      {/* Indexing Progress & Completion */}
      <IndexingDialog />

      {/* Library picker dialog (multiple libraries available) */}
      <Show when={library.showLibraryPicker()}>
        <div class="modal-overlay fixed inset-0 z-40 flex items-center justify-center">
          <div class="glass-modal rounded-xl p-6 w-96 animate-scale-in">
            <h2 class="text-lg font-semibold mb-4 text-center" style={{ color: "var(--text-primary)" }}>选择要打开的库</h2>
            <div class="flex flex-col gap-2 mb-4 max-h-60 overflow-y-auto">
              {library.restoredLibraries().map((lib) => (
                <button
                  class="text-left px-4 py-3 rounded-lg transition-colors w-full"
                  style={{ background: "var(--bg-tertiary)", color: "var(--text-primary)" }}
                  onClick={() => {
                    library.setActiveLibraryId(lib.id);
                    library.setShowLibraryPicker(false);
                    files.loadFiles(0);
                    tags.loadTags();
                  }}
                >
                  <span class="flex items-center gap-2.5">
                    <span class="flex-shrink-0" style={{ color: "var(--text-muted)" }}>
                      {lib.is_network ? <IconGlobe size={16} /> : <IconLibrary size={16} />}
                    </span>
                    <span class="flex-1 min-w-0">
                      <span class="block text-sm font-medium truncate">{lib.name}</span>
                      <span class="block text-2xs truncate" style={{ color: "var(--text-muted)" }}>
                        {lib.root_path} · {lib.file_count} 个文件
                      </span>
                    </span>
                  </span>
                </button>
              ))}
            </div>
            <button class="btn-secondary text-sm w-full" onClick={() => {
              library.setShowLibraryPicker(false);
            }}>
              取消
            </button>
          </div>
        </div>
      </Show>
    </div>
  );
}
