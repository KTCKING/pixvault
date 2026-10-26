import { Show, For, onMount, onCleanup, createSignal, createEffect } from "solid-js";
import type { JSX } from "solid-js";
import { useUIStore } from "@/stores/ui";
import { useFilesStore } from "@/stores/files";
import { useLibraryStore } from "@/stores/library";
import { useTagsStore } from "@/stores/tags";
import { api, COLOR_LABELS } from "@/lib/tauri-api";
import { open } from "@tauri-apps/plugin-dialog";
import {
  IconClose,
  IconCopy,
  IconFolder,
  IconFolderOpen,
  IconHeart,
  IconImage,
  IconLogOut,
  IconPalette,
  IconPencil,
  IconPlus,
  IconTrash,
  IconTrashX,
  IconUndo,
} from "@/components/icons";

interface MenuItem {
  label: string;
  icon?: JSX.Element;
  danger?: boolean;
  divider?: boolean;
  disabled?: boolean;
  onClick: () => void;
}

export function ContextMenu() {
  const ui = useUIStore();
  const files = useFilesStore();
  const library = useLibraryStore();
  const tags = useTagsStore();

  const menu = () => ui.contextMenu();
  const [position, setPosition] = createSignal({ x: 0, y: 0 });

  // Whenever a context menu is requested, position it at the cursor and clamp
  // it back inside the viewport once the element has rendered.
  createEffect(() => {
    const m = menu();
    if (!m) return;
    // Place it at the cursor immediately (so it never flashes at 0,0).
    setPosition({ x: m.x, y: m.y });
    // After the DOM node is mounted, clamp it to the viewport.
    requestAnimationFrame(() => {
      const node = document.getElementById("pixvault-context-menu");
      if (!node) return;
      const rect = node.getBoundingClientRect();
      let x = m.x;
      let y = m.y;
      if (x + rect.width > window.innerWidth) x = Math.max(0, window.innerWidth - rect.width - 8);
      if (y + rect.height > window.innerHeight) y = Math.max(0, window.innerHeight - rect.height - 8);
      setPosition({ x, y });
    });
  });

  const close = () => ui.setContextMenu(null);

  const closeOnOutside = (e: MouseEvent) => {
    const node = document.getElementById("pixvault-context-menu");
    if (node && e.target instanceof Node && node.contains(e.target)) return;
    close();
  };

  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") close();
  };

  onMount(() => {
    window.addEventListener("mousedown", closeOnOutside);
    window.addEventListener("keydown", onKey);
    window.addEventListener("blur", close);
    window.addEventListener("resize", close);
    onCleanup(() => {
      window.removeEventListener("mousedown", closeOnOutside);
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("blur", close);
      window.removeEventListener("resize", close);
    });
  });

  const buildItems = (): MenuItem[] => {
    const t = menu()?.target;
    if (!t) return [];

    if (t.kind === "files") {
      const paths = t.paths;
      const single = paths.length === 1;
      const items: MenuItem[] = [
        {
          label: single ? "打开图片" : `打开 ${paths.length} 张图片`,
          icon: <IconImage size={15} />,
          onClick: async () => {
            for (const p of paths) await api.openFile(p);
            close();
          },
        },
        {
          label: "复制",
          icon: <IconCopy size={15} />,
          onClick: async () => {
            try {
              await api.copyFilesToClipboard(paths);
              ui.showToast(paths.length > 1 ? `已复制 ${paths.length} 张图片到剪贴板` : "已复制到剪贴板");
            } catch (e) {
              console.error("复制失败:", e);
              ui.showToast("复制失败", "error");
            }
            close();
          },
        },
        {
          label: "打开所在文件夹",
          icon: <IconFolder size={15} />,
          disabled: !single,
          onClick: async () => {
            if (single) await api.openInExplorer(paths[0]);
            close();
          },
        },
        { label: "", divider: true, onClick: () => {} },
        {
          label: single
            ? (files.files().find((f) => f.path === paths[0])?.favorite ? "取消收藏" : "收藏")
            : "收藏",
          icon: <IconHeart size={15} />,
          onClick: async () => {
            const allFav = paths.every(
              (p) => files.files().find((f) => f.path === p)?.favorite
            );
            for (const p of paths) await files.setFavorite(p, !allFav);
            ui.showToast(allFav ? "已取消收藏" : "已收藏");
            close();
          },
        },
        { label: "颜色标记", divider: true, onClick: () => {} },
        ...COLOR_LABELS.map<MenuItem>((c) => ({
          label: c.name,
          icon: <span class="w-2.5 h-2.5 rounded-full" style={{ background: c.hex }} />,
          onClick: async () => {
            for (const p of paths) await files.setColor(p, c.name);
            ui.showToast(`已标记为 ${c.name}`);
            close();
          },
        })),
        {
          label: "清除颜色标记",
          icon: <IconClose size={15} />,
          onClick: async () => {
            for (const p of paths) await files.setColor(p, null);
            ui.showToast("已清除颜色标记");
            close();
          },
        },
        { label: "", divider: true, onClick: () => {} },
        {
          label: "移入回收站",
          icon: <IconTrash size={15} />,
          danger: true,
          onClick: () => {
            const n = paths.length;
            close();
            ui.requestConfirm({
              title: "移入回收站",
              message: n > 1 ? `确定要将选中的 ${n} 张图片移入回收站吗？` : "确定要将这张图片移入回收站吗？",
              confirmText: "移入回收站",
              danger: true,
              onConfirm: async () => {
                await files.trashFiles(paths);
                ui.showToast(n > 1 ? `已移入回收站 ${n} 张` : "已移入回收站");
              },
            });
          },
        },
      ];
      return items;
    }

    // trash (soft-deleted files) menu
    if (t.kind === "trash") {
      const paths = t.paths;
      // Right-click on the trash entry itself (no specific file) → empty-all.
      if (paths.length === 0) {
        return [
          {
            label: "打开回收站",
            icon: <IconTrash size={15} />,
            onClick: async () => {
              await files.setView({ kind: "trash" });
              close();
            },
          },
          {
            label: "清空回收站",
            icon: <IconTrashX size={15} />,
            danger: true,
            onClick: async () => {
              close();
              await files.loadDeletedFiles();
              const n = files.deletedFiles().length;
              if (n === 0) {
                ui.showToast("回收站已经是空的", "info");
                return;
              }
              ui.requestConfirm({
                title: "清空回收站",
                message: `确定要彻底删除回收站中的 ${n} 个文件吗？此操作不可恢复。`,
                confirmText: "彻底删除",
                danger: true,
                onConfirm: async () => {
                  await files.emptyTrash();
                  ui.showToast("回收站已清空");
                },
              });
            },
          },
        ];
      }
      const items: MenuItem[] = [
        {
          label: "恢复",
          icon: <IconUndo size={15} />,
          onClick: async () => {
            for (const p of paths) await files.restoreFile(p);
            ui.showToast(paths.length > 1 ? `已恢复 ${paths.length} 张` : "已恢复");
            close();
          },
        },
        {
          label: "彻底删除",
          icon: <IconTrashX size={15} />,
          danger: true,
          onClick: async () => {
            for (const p of paths) await files.purgeFile(p);
            ui.showToast("已彻底删除");
            close();
          },
        },
      ];
      return items;
    }

    // sidebar background menu (right-click on empty sidebar area)
    if (t.kind === "sidebar") {
      const openExistingLibrary = async () => {
        close();
        const selected = await open({
          directory: true,
          multiple: false,
          title: "选择文件夹作为图片库",
        });
        if (selected && typeof selected === "string") {
          await library.openLibrary(selected);
          await files.loadFiles(0);
          await tags.loadTags();
        }
      };

      const createNewLibrary = async () => {
        close();
        const parentDir = await open({
          directory: true,
          multiple: false,
          title: "选择新库要创建在哪个文件夹下",
        });
        if (!parentDir || typeof parentDir !== "string") return;
        ui.requestPrompt({
          title: "新建库",
          message: "输入新库的名称（将在所选文件夹下新建同名文件夹）",
          defaultValue: "新建图库",
          placeholder: "库名称",
          confirmText: "创建",
          onSubmit: async (name) => {
            const trimmed = name.trim();
            if (!trimmed) return;
            try {
              const res = await api.createLibrary(parentDir, trimmed);
              // Avoid duplicates if the same folder was already open.
              if (!library.libraries().find((l) => l.id === res.library.id)) {
                library.setLibraries((prev) => [...prev, res.library]);
              }
              library.setActiveLibraryId(res.library.id);
              await files.loadFiles(0);
              await tags.loadTags();
              ui.showToast(`已创建库「${res.library.name}」`);
            } catch (e) {
              console.error("新建库失败:", e);
              ui.showToast(typeof e === "string" ? e : "新建库失败", "error");
            }
          },
        });
      };

      return [
        {
          label: "新建库",
          icon: <IconPlus size={15} />,
          onClick: createNewLibrary,
        },
        {
          label: "打开已有库",
          icon: <IconFolderOpen size={15} />,
          onClick: openExistingLibrary,
        },
      ];
    }

    // library menu
    const items: MenuItem[] = [
      {
        label: "在资源管理器中打开",
        icon: <IconFolder size={15} />,
        onClick: async () => {
          const lib = library.libraries().find((l) => l.id === t.libraryId);
          if (lib) await api.openInExplorer(lib.root_path);
          close();
        },
      },
      {
        label: "重命名",
        icon: <IconPencil size={15} />,
        onClick: () => {
          const lib = library.libraries().find((l) => l.id === t.libraryId);
          close();
          if (!lib) return;
          ui.requestPrompt({
            title: "重命名库",
            message: "输入新的库名称",
            defaultValue: lib.name,
            placeholder: "库名称",
            confirmText: "重命名",
            onSubmit: async (name) => {
              const trimmed = name.trim();
              if (!trimmed || trimmed === lib.name) return;
              await api.renameLibrary(t.libraryId, trimmed);
              library.setLibraries(
                library.libraries().map((l) =>
                  l.id === t.libraryId ? { ...l, name: trimmed } : l
                )
              );
              ui.showToast("已重命名");
            },
          });
        },
      },
      { label: "", divider: true, onClick: () => {} },
      {
        label: "移除库（保留文件）",
        icon: <IconLogOut size={15} />,
        onClick: async () => {
          await api.deleteLibrary(t.libraryId, false);
          library.setLibraries(library.libraries().filter((l) => l.id !== t.libraryId));
          if (library.activeLibraryId() === t.libraryId) {
            library.setActiveLibraryId(library.libraries()[0]?.id ?? null);
          }
          files.loadFiles(0);
          close();
        },
      },
      {
        label: "删除库及所有文件",
        icon: <IconTrashX size={15} />,
        danger: true,
        onClick: () => {
          const lib = library.libraries().find((l) => l.id === t.libraryId);
          close();
          if (!lib) return;
          ui.requestConfirm({
            title: "删除库",
            message: `确定要彻底删除库「${lib.name}」吗？\n\n这将永久删除该文件夹下的所有文件（含 .pixvault 数据），且无法恢复！`,
            confirmText: "永久删除",
            danger: true,
            onConfirm: async () => {
              await api.deleteLibrary(t.libraryId, true);
              library.setLibraries(library.libraries().filter((l) => l.id !== t.libraryId));
              if (library.activeLibraryId() === t.libraryId) {
                library.setActiveLibraryId(library.libraries()[0]?.id ?? null);
              }
              files.loadFiles(0);
              ui.showToast("库已删除");
            },
          });
        },
      },
    ];
    return items;
  };

  return (
    <Show when={menu()}>
      <div
        id="pixvault-context-menu"
        class="context-menu fixed z-50 min-w-48 animate-scale-in"
        style={{
          left: `${position().x}px`,
          top: `${position().y}px`,
        }}
        onClick={(e) => e.stopPropagation()}
        onContextMenu={(e) => e.preventDefault()}
      >
        <For each={buildItems()}>
          {(item) =>
            item.divider ? (
              <div class="my-1" style={{ "border-top": "1px solid var(--border-color)" }} />
            ) : (
              <button
                class="flex items-center gap-2 w-full px-3 py-1.5 text-xs text-left transition-colors"
                classList={{
                  "opacity-40 cursor-not-allowed": item.disabled,
                  "hover:bg-red-500/10": item.danger && !item.disabled,
                }}
                style={{
                  color: item.danger ? "var(--danger, #e5484d)" : "var(--text-primary)",
                }}
                onClick={item.onClick}
              >
                <span class="w-4 flex justify-center">{item.icon}</span>
                <span>{item.label}</span>
              </button>
            )
          }
        </For>
      </div>
    </Show>
  );
}
