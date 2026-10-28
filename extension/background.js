/**
 * PixVault 网页摘取 —— 后台服务
 *
 * 分工：content script 负责「页面上有什么」，这里负责「把字节拿下来并送进
 * PixVault」。为什么不在页面里直接 POST 到 127.0.0.1？因为页面的 CSP
 * （connect-src）经常不允许，而扩展的后台不受此限制。
 *
 * 三件容易被忽略的事：
 *  - **失败重试**：一次存 30 张图，偶尔有一张卡在 5xx 或超时。以前整批只跑一遍，
 *    失败就得重新勾选重存；现在每张最多试 3 次，且记住失败项供面板「重试」。
 *  - **跨帧汇总**：manifest 开了 `all_frames`，每个 iframe 里的图归那个帧自己扫。
 *    这里维护一张「活着的帧」表，顶层帧来问时逐帧收集再合并（按 URL 去重）。
 *  - **4xx 不重试**：库没开、body 是空的、名字非法 —— 这些重试一百次也是同一个结果。
 */

const DEFAULT_CONFIG = {
  port: 41596,
  token: "",
  /** 按站点建子文件夹：存到 库/example.com/ 下 */
  folderBySite: true,
  /** 关闭「按站点建文件夹」时用这个固定子文件夹（空 = 库根目录） */
  folder: "",
  /** 上次在采集面板里选的存放位置：`@auto` = 按站点自动，`""` = 库根，其它 = 相对路径 */
  pickFolder: "@auto",
  /** 上次在采集面板里填的标签 */
  pickTags: [],
  /** 小于这个边长的图直接忽略（像素） */
  minSize: 64,
  /** 悬停图片时显示采集按钮 */
  hoverButton: true,
  /** 采集面板里默认包含视频 */
  includeVideo: true,
};

const VIDEO_EXT = /\.(mp4|m4v|webm|mov|mkv|avi|wmv|flv|ogv|mpg|mpeg|3gp)(\?|#|$)/i;

/** 单张最多试几次（含第一次），以及两次之间的等待 */
const RETRY_ATTEMPTS = 3;
const RETRY_DELAY_MS = [400, 900];

async function getConfig() {
  return chrome.storage.local.get(DEFAULT_CONFIG);
}

async function api(path, init = {}) {
  const cfg = await getConfig();
  const headers = Object.assign({}, init.headers || {});
  if (cfg.token) headers["x-pixvault-token"] = cfg.token;
  const res = await fetch(`http://127.0.0.1:${cfg.port}${path}`, {
    ...init,
    headers,
  });
  const text = await res.text();
  let json = null;
  try {
    json = JSON.parse(text);
  } catch {
    /* 非 JSON 响应（比如被中间设备拦了）就按纯文本报错 */
  }
  if (!res.ok) {
    throw new Error((json && json.error) || text || `HTTP ${res.status}`);
  }
  return json;
}

async function ping() {
  try {
    const r = await api("/api/ping");
    return { ok: true, version: r.version, library: r.library };
  } catch (e) {
    return { ok: false, error: String((e && e.message) || e) };
  }
}

/** 一串任务排队跑：同时发 20 个请求既容易打爆内存，也会让后端来回抢锁。 */
let chain = Promise.resolve();
function serial(task) {
  const p = chain.then(task, task);
  chain = p.catch(() => {});
  return p;
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function siteFolder(pageUrl) {
  try {
    const host = new URL(pageUrl).hostname.replace(/^www\./, "");
    // 只保留能当文件夹名的字符，其它一律换成下划线
    return host.replace(/[<>:"/\\|?*\x00-\x1f]/g, "_").slice(0, 60);
  } catch {
    return "";
  }
}

/**
 * 面板选的值 → 真实落点。`@auto` 交给「按站点」那条老规则，其余原样（空 = 库根）。
 */
function resolveFolder(cfg, pageUrl, override) {
  const pick = override === undefined || override === null ? cfg.pickFolder : override;
  if (pick === "@auto") {
    return cfg.folderBySite ? siteFolder(pageUrl) : String(cfg.folder || "").trim();
  }
  return String(pick || "").replace(/^\/+|\/+$/g, "");
}

function guessName(url, fallbackExt) {
  try {
    const u = new URL(url);
    let base = decodeURIComponent(u.pathname.split("/").pop() || "");
    base = base.split("?")[0];
    if (!base) base = "untitled";
    if (!/\.[a-z0-9]{2,5}$/i.test(base) && fallbackExt) base += `.${fallbackExt}`;
    return base;
  } catch {
    return `untitled.${fallbackExt || "jpg"}`;
  }
}

function base64ToBlob(base64, mime) {
  const bin = atob(base64);
  const len = bin.length;
  const bytes = new Uint8Array(len);
  for (let i = 0; i < len; i++) bytes[i] = bin.charCodeAt(i);
  return new Blob([bytes], { type: mime || "application/octet-stream" });
}

/**
 * 把一张图 / 一段视频变成 Blob。
 *
 * `blob:` 地址属于页面的 blob store，后台取不到，只能让 content script 去抓
 * （它在页面上下文里）；而且还必须是**产出这个 blob 的那一帧**，所以要带上 frameId。
 * 其余情况后台自己 fetch —— 带上站点 cookie，图床防盗链和需要登录的图都能下来。
 */
async function fetchBlob(item) {
  const url = item.url;
  if (url.startsWith("blob:")) {
    const opts = item.frameId === undefined ? undefined : { frameId: item.frameId };
    const resp = await chrome.tabs.sendMessage(item.tabId, { type: "pixvault:grab", url }, opts);
    if (!resp || !resp.ok) throw new Error(resp?.error || "抓取页面内的 blob 失败");
    return { blob: base64ToBlob(resp.base64, resp.mime), mime: resp.mime };
  }

  // 大视频先探一下长度，超上限直接放弃，别把浏览器内存拖垮
  const res = await fetch(url, { credentials: "include" });
  if (!res.ok) throw new Error(`下载失败 HTTP ${res.status}`);
  const blob = await res.blob();
  return { blob, mime: blob.type || item.mime || "" };
}

async function ingestOne(item, pageUrl, folder, tags) {
  const { blob, mime } = await fetchBlob(item);
  if (blob.size === 0) throw new Error("拿到的是空文件（可能需要登录或被防盗链拦了）");

  const qs = new URLSearchParams();
  qs.set("name", item.name || guessName(item.url, ""));
  if (pageUrl) qs.set("page", pageUrl);
  if (folder) qs.set("folder", folder);
  if (tags && tags.length) qs.set("tags", tags.join(","));

  return api(`/api/ingest?${qs.toString()}`, {
    method: "POST",
    headers: { "content-type": mime || "application/octet-stream" },
    body: blob,
  });
}

/** 4xx 是「说明白了的拒绝」：库没开、body 空、名字非法 —— 重试只是白白多等一会。 */
function isRetryable(err) {
  const s = String((err && err.message) || err);
  if (/HTTP 4\d\d/.test(s)) return false;
  if (/没有打开图库|请求体是空的|未知接口/.test(s)) return false;
  return true;
}

async function ingestWithRetry(item, pageUrl, folder, tags, onAttempt) {
  let lastErr;
  for (let attempt = 0; attempt < RETRY_ATTEMPTS; attempt++) {
    try {
      return await ingestOne(item, pageUrl, folder, tags);
    } catch (e) {
      lastErr = e;
      const more = attempt < RETRY_ATTEMPTS - 1 && isRetryable(e);
      if (!more) break;
      onAttempt?.(attempt + 1);
      await sleep(RETRY_DELAY_MS[attempt] ?? 1200);
    }
  }
  throw lastErr;
}

function badge(text) {
  chrome.action.setBadgeText({ text: text || "" }).catch(() => {});
  if (text) {
    chrome.action
      .setBadgeBackgroundColor({ color: "#1a1a1a" })
      .catch(() => {});
  }
}

/** 进度同时推给发起采集的那个标签页和 popup；没有接收方时静默失败即可。 */
function broadcast(msg, tabId) {
  chrome.runtime.sendMessage(msg).catch(() => {});
  if (tabId) chrome.tabs.sendMessage(tabId, msg).catch(() => {});
}

/** 每个标签页最近一批的失败项，面板上的「重试」就靠它。 */
const lastFailed = new Map(); // tabId -> { items, pageUrl, folder, tags }

async function runBatch(items, pageUrl, tabId, opts = {}) {
  const cfg = await getConfig();
  const folder = resolveFolder(cfg, pageUrl, opts.folder);
  const tags = Array.isArray(opts.tags) ? opts.tags : Array.isArray(cfg.pickTags) ? cfg.pickTags : [];

  let ok = 0;
  const errors = [];
  const failed = [];
  for (let i = 0; i < items.length; i++) {
    const item = { ...items[i], tabId };
    const name = item.name || guessName(item.url, "");
    broadcast(
      { type: "pixvault:progress", current: i + 1, total: items.length, name },
      tabId,
    );
    badge(`${i + 1}/${items.length}`);
    try {
      await ingestWithRetry(item, pageUrl, folder, tags, (retry) => {
        broadcast(
          { type: "pixvault:progress", current: i + 1, total: items.length, name, retrying: retry },
          tabId,
        );
      });
      ok += 1;
    } catch (e) {
      const msg = (e && e.message) || String(e);
      errors.push(`${name}：${msg}`);
      failed.push({ url: item.url, name, frameId: item.frameId });
    }
  }
  badge("");
  if (tabId !== undefined && tabId !== null) {
    if (failed.length > 0) lastFailed.set(tabId, { items: failed, pageUrl, folder, tags });
    else lastFailed.delete(tabId);
  }
  broadcast(
    { type: "pixvault:done", ok, fail: failed.length, errors, total: items.length },
    tabId,
  );
  return { ok, fail: failed.length, errors };
}

/** 标签页 → 活着的帧集合。content script 每次加载都会 hello 一次。 */
const frames = new Map();

function rememberFrame(tabId, frameId) {
  if (tabId === undefined || tabId === null || frameId === undefined || frameId === null) return;
  let set = frames.get(tabId);
  if (!set) {
    set = new Set();
    frames.set(tabId, set);
  }
  set.add(frameId);
}

/** 收齐这个标签页里所有活着的帧各自的素材，按 URL 去重（顶层帧优先）。 */
async function scanAll(tabId) {
  const ids = [...(frames.get(tabId) || new Set())];
  if (!ids.includes(0)) ids.unshift(0);
  ids.sort((a, b) => a - b);

  const perFrame = await Promise.all(
    ids.map(async (frameId) => {
      try {
        const r = await chrome.tabs.sendMessage(tabId, { type: "pixvault:scan" }, { frameId });
        return (r && Array.isArray(r.items) ? r.items : []).map((it) => ({ ...it, frameId }));
      } catch {
        // 那一帧已经不在了（导航 / 被移除），跳过就行
        return [];
      }
    }),
  );

  const seen = new Set();
  const out = [];
  for (const list of perFrame) {
    for (const it of list) {
      if (!it || !it.url || seen.has(it.url)) continue;
      seen.add(it.url);
      out.push(it);
    }
  }
  return out;
}

chrome.tabs.onRemoved.addListener((tabId) => {
  frames.delete(tabId);
  lastFailed.delete(tabId);
});

chrome.tabs.onUpdated.addListener((tabId, info) => {
  // 主文档重新加载：旧的 frameId 全部作废，重新等它们 hello
  if (info.status === "loading") frames.delete(tabId);
});

// ───────────────────────── 右键菜单 ─────────────────────────

chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.removeAll(() => {
    chrome.contextMenus.create({
      id: "pixvault-save",
      title: "保存到 PixVault",
      contexts: ["image", "video"],
    });
  });
});

chrome.contextMenus.onClicked.addListener((info, tab) => {
  if (info.menuItemId !== "pixvault-save") return;
  const url = info.srcUrl;
  if (!url || !tab) return;
  const isVideo = Boolean(info.mediaType === "video" || VIDEO_EXT.test(url));
  void serial(async () => {
    // 右键菜单没有面板，沿用上一次在面板里选的文件夹/标签
    await runBatch([{ url, name: guessName(url, isVideo ? "mp4" : "jpg") }], tab.url, tab.id);
  });
});

// ───────────────────────── 消息路由 ─────────────────────────

chrome.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  if (!msg || typeof msg.type !== "string") return;

  if (msg.type === "pixvault:hello") {
    // 子帧也要登记（all_frames），顶层帧汇总扫描时才知道该问谁
    if (sender.tab) rememberFrame(sender.tab.id, sender.frameId);
    sendResponse({ ok: true, frameId: sender.frameId ?? 0 });
    return true;
  }

  if (msg.type === "pixvault:ping") {
    ping().then(sendResponse);
    return true;
  }

  if (msg.type === "pixvault:config") {
    getConfig().then(sendResponse);
    return true;
  }

  if (msg.type === "pixvault:meta") {
    (async () => {
      const cfg = await getConfig();
      const site = siteFolder(msg.pageUrl || "");
      const p = await ping();
      if (!p.ok) {
        sendResponse({ ok: false, error: p.error, folders: [], tags: [], siteFolder: site });
        return;
      }
      try {
        const r = await api("/api/meta");
        sendResponse({
          ok: true,
          library: r.library,
          folders: r.folders || [],
          tags: r.tags || [],
          siteFolder: site,
          autoFolder: resolveFolder(cfg, msg.pageUrl || "", "@auto"),
        });
      } catch (e) {
        sendResponse({ ok: false, error: String((e && e.message) || e), folders: [], tags: [], siteFolder: site });
      }
    })();
    return true;
  }

  if (msg.type === "pixvault:scan-all") {
    const tabId = sender.tab?.id;
    if (tabId === undefined || tabId === null) {
      sendResponse({ items: [] });
      return true;
    }
    scanAll(tabId)
      .then((items) => sendResponse({ items }))
      .catch((e) => sendResponse({ items: [], error: String(e) }));
    return true;
  }

  // 子帧按 Alt+S 时把「打开面板」这件事转交给顶层帧
  if (msg.type === "pixvault:open-panel") {
    const tabId = sender.tab?.id;
    if (tabId !== undefined && tabId !== null) {
      chrome.tabs
        .sendMessage(tabId, { type: "pixvault:toggle-panel" }, { frameId: 0 })
        .catch(() => {});
    }
    sendResponse({ ok: true });
    return true;
  }

  if (msg.type === "pixvault:save") {
    const tabId = sender.tab?.id;
    const pageUrl = msg.pageUrl || sender.tab?.url || "";
    const items = Array.isArray(msg.items) ? msg.items : [];
    if (items.length === 0) {
      sendResponse({ started: false, error: "没有选中任何内容" });
      return true;
    }

    // 先把结果告诉调用方，真正干活放到后台串行队列里 —— 一次存 50 张图可能要
    // 一分多钟，消息通道等不起。
    serial(async () => {
      const p = await ping();
      if (!p.ok) {
        broadcast(
          {
            type: "pixvault:done",
            ok: 0,
            fail: items.length,
            errors: [`连不上 PixVault：${p.error}`],
            total: items.length,
          },
          tabId,
        );
        return;
      }
      await runBatch(items, pageUrl, tabId, { folder: msg.folder, tags: msg.tags });
    });

    sendResponse({ started: true, total: items.length });
    return true;
  }

  if (msg.type === "pixvault:retry-failed") {
    const tabId = sender.tab?.id;
    const prev = tabId === undefined ? null : lastFailed.get(tabId);
    if (!prev || prev.items.length === 0) {
      sendResponse({ started: false, error: "没有失败项" });
      return true;
    }
    const batch = { ...prev };
    serial(async () => {
      await runBatch(batch.items, batch.pageUrl, tabId, {
        folder: batch.folder,
        tags: batch.tags,
      });
    });
    sendResponse({ started: true, total: batch.items.length });
    return true;
  }
});

// 端口 / token 改了之后让已打开的标签页重新渲染状态（content 的角标）
chrome.storage.onChanged.addListener((changes, area) => {
  if (area !== "local") return;
  chrome.tabs.query({}).then((tabs) => {
    for (const t of tabs) {
      chrome.tabs
        .sendMessage(t.id, { type: "pixvault:config-changed" })
        .catch(() => {});
    }
  });
});
