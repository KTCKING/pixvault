import raw from "../../CHANGELOG.md?raw";

/** 一个版本里的一组条目，例如「新增」下的若干条。 */
export interface ReleaseGroup {
  /** 分组标题（`新增` / `修复` / `调整`）；没有小标题时是空串。 */
  title: string;
  items: string[];
}

export interface ReleaseNote {
  /** 只保留数字部分，例如 `0.8.0`。 */
  version: string;
  /** `YYYY-MM-DD`，标题里没写日期时是空串。 */
  date: string;
  groups: ReleaseGroup[];
}

/**
 * 把 `CHANGELOG.md` 解析成结构化数据。
 *
 * 只认三种行，格式在源文件里已经约定好了：
 * - `## 0.8.0 — 2026-11-10` 开一个新版本
 * - `### 新增` 开一组
 * - `- 一句话` 一条
 *
 * 其余（顶层 `# 标题`、说明段落、空行）一律跳过。故意不做完整的 Markdown
 * 解析 —— 这个文件是给设置面板看的，条目多一层嵌套也显示不出来。
 */
export function parseChangelog(markdown: string): ReleaseNote[] {
  const releases: ReleaseNote[] = [];
  let current: ReleaseNote | null = null;
  let group: ReleaseGroup | null = null;

  const clean = (text: string) =>
    text
      .replace(/`/g, "")
      .replace(/\*\*(.+?)\*\*/g, "$1")
      .replace(/\s+/g, " ")
      .trim();

  for (const line of markdown.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (!trimmed) continue;

    if (trimmed.startsWith("## ")) {
      const heading = trimmed.slice(3);
      const m = heading.match(/^v?(\d+(?:\.\d+)*)\s*(?:[—–-]\s*)?(\d{4}-\d{1,2}-\d{1,2})?/);
      if (!m) {
        current = null;
        group = null;
        continue;
      }
      current = { version: m[1], date: m[2] ?? "", groups: [] };
      group = null;
      releases.push(current);
      continue;
    }

    if (trimmed.startsWith("### ")) {
      if (!current) continue;
      group = { title: clean(trimmed.slice(4)), items: [] };
      current.groups.push(group);
      continue;
    }

    if (trimmed.startsWith("- ")) {
      if (!current) continue;
      if (!group) {
        group = { title: "", items: [] };
        current.groups.push(group);
      }
      group.items.push(clean(trimmed.slice(2)));
      continue;
    }

    // 折行的续写：接到上一条后面，避免长句被切成两半。
    if (current && group && group.items.length > 0 && !trimmed.startsWith("#")) {
      const last = group.items.length - 1;
      group.items[last] = clean(`${group.items[last]} ${trimmed}`);
    }
  }

  return releases;
}

/** 版本从新到旧（源文件本身就是这个顺序，这里再按语义化版本兜一次底）。 */
export const CHANGELOG: ReleaseNote[] = parseChangelog(raw).sort((a, b) => {
  const pa = a.version.split(".").map(Number);
  const pb = b.version.split(".").map(Number);
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const d = (pb[i] || 0) - (pa[i] || 0);
    if (d !== 0) return d;
  }
  return 0;
});

/** 当前版本对应的那条更新内容；找不到就返回 null。 */
export function notesFor(version: string): ReleaseNote | null {
  const v = version.replace(/^v/, "");
  return CHANGELOG.find((r) => r.version === v) ?? null;
}
