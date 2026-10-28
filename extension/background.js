/**
 * PixVault 网页摘取 —— 后台服务
 *
 * 分工：content script 负责「页面上有什么」，这里负责「把字节拿下来并送进
 * PixVault」。为什么不在页面里直接 POST 到 127.0.0.1？因为页面的 CSP
 * （connect-src）经常不允许，而扩展的后台不受此限制。
 */

const DEFAULT_CONFIG = {
  port: 41596,
  token: "",
  /** 按站点建子文件夹：存到 库/example.com/ 下 */
  folderBySite: true,
  /** 关闭「按站点建文件夹」时用这个固定子文件夹（空 = 库根目录） */
  folder: "",
  /** 小于这个边长的图直接忽略（像素） */
  minSize: 64,
  /** 悬停图片时显示采集按钮 */
  hoverButton: true,
  /** 采集面板里默认包含视频 */
  includeVideo: true,
};

const VIDEO_EXT = /\.(mp4|m4v|webm|mov|mkv|avi|wmv|flv|ogv|mpg|mpeg|3gp)(\?|#|$)/i;

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

function siteFolder(pageUrl) {
  try {
    const host = new URL(pageUrl).hostname.replace(/^www\./, "");
    // 只保留能当文件夹名的字符，其它一律换成下划线
    return host.replace(/[<>:"/\\|?*\x00-\x1f]/g, "_").slice(0, 60);
  } catch {
    return "";
  }
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
 * （它在页面上下文里）。其余情况后台自己 fetch —— 带上站点 cookie，图床防盗链
 * 和需要登录的图都能下来。
 */
async function fetchBlob(item, pageUrl) {
  const url = item.url;
  if (url.startsWith("blob:")) {
    const resp = await chrome.tabs.sendMessage(item.tabId, {
      type: "pixvault:grab",
      url,
    });
    if (!resp || !resp.ok) throw new Error(resp?.error || "抓取页面内的 blob 失败");
    return { blob: base64ToBlob(resp.base64, resp.mime), mime: resp.mime };
  }

  // 大视频先探一下长度，超上限直接放弃，别把浏览器内存拖垮
  const res = await fetch(url, { credentials: "include" });
  if (!res.ok) throw new Error(`下载失败 HTTP ${res.status}`);
  const blob = await res.blob();
  return { blob, mime: blob.type || item.mime || "" };
}

async function ingestOne(item, pageUrl, folder) {
  const { blob, mime } = await fetchBlob(item, pageUrl);
  if (blob.size === 0) throw new Error("拿到的是空文件（可能需要登录或被防盗链拦了）");

  const cfg = await getConfig();
  const qs = new URLSearchParams();
  qs.set("name", item.name || guessName(item.url, ""));
  if (pageUrl) qs.set("page", pageUrl);
  if (folder) qs.set("folder", folder);

  return api(`/api/ingest?${qs.toString()}`, {
    method: "POST",
    headers: { "content-type": mime || "application/octet-stream" },
    body: blob,
  });
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

async function runBatch(items, pageUrl, tabId) {
  const cfg = await getConfig();
  const folder = cfg.folderBySite ? siteFolder(pageUrl) : cfg.folder.trim();

  let ok = 0;
  const errors = [];
  for (let i = 0; i < items.length; i++) {
    const item = { ...items[i], tabId };
    broadcast(
      {
        type: "pixvault:progress",
        current: i + 1,
        total: items.length,
        name: item.name || guessName(item.url, ""),
      },
      tabId,
    );
    badge(`${i + 1}/${items.length}`);
    try {
      await ingestOne(item, pageUrl, folder);
      ok += 1;
    } catch (e) {
      errors.push(`${item.name || item.url}：${(e && e.message) || e}`);
    }
  }
  badge("");
  broadcast({ type: "pixvault:done", ok, fail: errors.length, errors, total: items.length }, tabId);
  return { ok, fail: errors.length, errors };
}

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
    await runBatch([{ url, name: guessName(url, isVideo ? "mp4" : "jpg") }], tab.url, tab.id);
  });
});

// ───────────────────────── 消息路由 ─────────────────────────

chrome.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  if (!msg || typeof msg.type !== "string") return;

  if (msg.type === "pixvault:ping") {
    ping().then(sendResponse);
    return true;
  }

  if (msg.type === "pixvault:config") {
    getConfig().then(sendResponse);
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
      await runBatch(items, pageUrl, tabId);
    });

    sendResponse({ started: true, total: items.length });
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
