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
      // 回收站跨库，每条要带着自己的来源库回去执行，不能用当前活动库。
      const libIdOf = (p: string) =>
        files.deletedFiles().find((f) => f.path === p)?.library_id;
      const alsoDisk = () => ui.purgeDeletesDisk();
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
              const disk = alsoDisk();
              ui.requestConfirm({
                title: "清空回收站",
                message: disk
                  ? `将删除所有库回收站里的 ${n} 个文件，并连同磁盘上的原文件一起删除。此操作不可恢复。`
                  : `将把 ${n} 个文件从库中移除，磁盘上的原文件会保留。`,
                confirmText: disk ? "删除并清理磁盘" : "从库中移除",
                danger: true,
                onConfirm: async () => {
                  const r = await files.emptyTrash();
                  if (r.failed.length > 0) {
                    ui.showToast(`已清空 ${r.removed} 个，${r.failed.length} 个未能删除`, "error");
                  } else {
                    ui.showToast("回收站已清空");
                  }
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
            for (const p of paths) await files.restoreFile(p, libIdOf(p));
            ui.showToast(paths.length > 1 ? `已恢复 ${paths.length} 张` : "已恢复");
            close();
          },
        },
        {
          label: "彻底删除",
          icon: <IconTrashX size={15} />,
          danger: true,
          onClick: async () => {
            close();
            const disk = alsoDisk();
            ui.requestConfirm({
              title: "彻底删除",
              message: disk
                ? `将删除 ${paths.length} 个文件及其磁盘上的原文件，不可恢复。`
                : `将把 ${paths.length} 个文件从库中移除，磁盘上的原文件保留。`,
              confirmText: disk ? "删除并清理磁盘" : "从库中移除",
              danger: true,
              onConfirm: async () => {
                for (const p of paths) await files.purgeFile(p, libIdOf(p));
                ui.showToast(disk ? "已删除并清理磁盘文件" : "已从库中移除");
              },
            });
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

    // library menu（右键侧栏「资源库」里的某一条）
    const lib = library.libraries().find((l) => l.id === t.libraryId);
    // 库刚被别处移除时（菜单还悬着）没有可操作对象，直接给空菜单。
    if (!lib) return [];

    /** 库从列表消失之后收拾现场：当前库没了就回到「所有图片」并重拉数据。 */
    const afterLibraryGone = async (id: string) => {
      const wasActive = library.activeLibraryId() === id;
      library.setLibraries(library.libraries().filter((l) => l.id !== id));
      if (!wasActive) return;
      const next = library.libraries()[0];
      library.setActiveLibraryId(next?.id ?? null);
      if (!next) {
        // 一个库都不剩了：把残留的列表清干净，别让已删库的图还挂在界面上。
        files.setFiles([]);
        files.setTotalFiles(0);
        return;
      }
      // 视图可能正停在刚消失那个库的文件夹 / 标签上，回到「所有图片」最稳。
      // 这几个 reload 失败也不该影响「删除成功」这个主结果，所以各自吞掉。
      await files.setView({ kind: "all" }).catch(() => {});
      await files.loadFolderTree().catch(() => {});
      await tags.loadTags().catch(() => {});
      void files.loadViewCounts().catch(() => {});
    };

    const items: MenuItem[] = [
      {
        label: "重命名",
        icon: <IconPencil size={15} />,
        onClick: () => {
          close();
          ui.requestPrompt({
            title: "重命名库",
            message: "只改图库里显示的名字，磁盘上的文件夹名不变。",
            defaultValue: lib.name,
            placeholder: "库名称",
            confirmText: "保存",
            onSubmit: async (input) => {
              const name = input.trim();
              if (!name || name === lib.name) return;
              if (/[/\\:*?"<>|]/.test(name)) {
                ui.showToast('库名不能包含 \\ / : * ? " < > | 等字符', "error");
                return;
              }
              try {
                await api.renameLibrary(lib.id, name);
                library.setLibraries(
                  library.libraries().map((l) => (l.id === lib.id ? { ...l, name } : l))
                );
                ui.showToast(`已重命名为「${name}」`);
              } catch (e) {
                ui.showToast(typeof e === "string" ? e : "重命名失败", "error");
              }
            },
          });
        },
      },
      {
        label: "在资源管理器中打开",
        icon: <IconFolder size={15} />,
        onClick: async () => {
          await api.openInExplorer(lib.root_path);
          close();
        },
      },
      { label: "", divider: true, onClick: () => {} },
      {
        label: "从列表移除（文件保留）",
        icon: <IconLogOut size={15} />,
        onClick: async () => {
          close();
          try {
            await api.deleteLibrary(lib.id, false);
            await afterLibraryGone(lib.id);
            ui.showToast("已从列表移除，文件仍在原处");
          } catch (e) {
            ui.showToast(typeof e === "string" ? e : "移除失败", "error");
          }
        },
      },
      {
        label: "删除库及所有文件",
        icon: <IconTrashX size={15} />,
        danger: true,
        onClick: () => {
          close();
          // 这一步删的是用户自己的文件夹，而且不进回收站 —— 一个「确定」
          // 按钮扛不住手滑，要求逐字输入库名才执行（后端也会复核）。
          ui.requestPrompt({
            title: `删除库「${lib.name}」`,
            message:
              `会永久删除下面这个文件夹` +
              (lib.file_count > 0 ? `连同里面的 ${lib.file_count} 个文件` : "") +
              "，不进回收站、无法恢复：\n" +
              `${lib.root_path}\n\n` +
              "如果这是原地读取的 Eagle / Pixcall 库，删掉的就是你原本的素材目录。\n" +
              `确认请原样输入库名「${lib.name}」；只想让它从列表消失，请改用「从列表移除」。`,
            placeholder: "输入库名以确认",
            confirmText: "永久删除",
            onSubmit: async (input) => {
              if (input.trim() !== lib.name) {
                ui.showToast("库名不匹配，已取消", "error");
                return;
              }
              try {
                await api.deleteLibrary(lib.id, true, lib.name);
                await afterLibraryGone(lib.id);
                ui.showToast(`已删除库「${lib.name}」及其文件`);
              } catch (e) {
                ui.showToast(typeof e === "string" ? e : "删除失败", "error");
              }
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
