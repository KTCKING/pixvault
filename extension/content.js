/**
 * PixVault 网页摘取 —— 页面侧
 *
 * 干三件事：
 *  1. 扫描当前页面所有能存的图片 / 视频（含 srcset、懒加载属性、背景图）；
 *  2. 提供一个采集面板（Alt+S 或点扩展图标）让人勾选；
 *  3. 悬停在图片上时右上角浮一个采集按钮，一键存单张。
 *
 * 面板整体放在 Shadow DOM 里：页面的 CSS 再野也影响不到它，它也不往外漏样式。
 */

(() => {
  if (window.__pixvaultLoaded) return;
  window.__pixvaultLoaded = true;

  const MEDIA_RE = /\.(jpe?g|png|gif|webp|avif|bmp|svg|tiff?|ico|heic|mp4|m4v|webm|mov|mkv|avi|wmv|flv|ogv|mpg|mpeg)(\?|#|$)/i;
  const VIDEO_RE = /\.(mp4|m4v|webm|mov|mkv|avi|wmv|flv|ogv|mpg|mpeg)(\?|#|$)/i;

  let config = {
    minSize: 64,
    hoverButton: true,
    includeVideo: true,
  };

  /** 面板 UI 状态 */
  let media = [];
  let selected = new Set();
  let busy = false;
  let panelOpen = false;

  // ───────────────────────── 扫描 ─────────────────────────

  const absUrl = (u) => {
    if (!u) return null;
    const s = String(u).trim();
    if (!s || s.startsWith("javascript:")) return null;
    try {
      return new URL(s, location.href).href;
    } catch {
      return null;
    }
  };

  /** srcset 里挑最大的那档。`a.jpg 1x, b.jpg 2x` 与 `a.jpg 800w` 两种写法都有。 */
  function largestSrcset(srcset) {
    let best = null;
    let bestWeight = -1;
    for (const part of String(srcset).split(",")) {
      const seg = part.trim();
      if (!seg) continue;
      const bits = seg.split(/\s+/);
      const url = bits[0];
      const desc = bits[1] || "";
      let weight = 0;
      if (desc.endsWith("w")) weight = parseInt(desc, 10) || 0;
      else if (desc.endsWith("x")) weight = (parseFloat(desc) || 1) * 1000;
      if (url && weight >= bestWeight) {
        best = url;
        bestWeight = weight;
      }
    }
    return best;
  }

  function guessName(url, isVideo) {
    try {
      const u = new URL(url);
      let base = decodeURIComponent((u.pathname.split("/").pop() || "").split("?")[0]);
      if (!base) base = "untitled";
      if (!/\.[a-z0-9]{2,5}$/i.test(base)) base += isVideo ? ".mp4" : ".jpg";
      return base;
    } catch {
      return isVideo ? "untitled.mp4" : "untitled.jpg";
    }
  }

  function collectMedia() {
    const map = new Map(); // url -> item（同一张图在页面上出现多次只留一条）
    const push = (rawUrl, extra) => {
      const url = absUrl(rawUrl);
      if (!url) return;
      if (url.startsWith("data:")) {
        if (!url.startsWith("data:image/") || url.length < 512) return; // 1x1 占位图
      } else if (!MEDIA_RE.test(url) && !url.startsWith("blob:")) {
        // 没有扩展名的图片地址（CDN 常见）也留着，交给后端按 mime 补扩展名
        if (!extra?.force) return;
      }
      if (map.has(url)) return;
      map.set(url, { url, ...extra });
    };

    for (const img of document.images) {
      const srcset = img.getAttribute("srcset") || img.getAttribute("data-srcset");
      const url =
        img.currentSrc ||
        img.src ||
        (srcset && largestSrcset(srcset)) ||
        img.getAttribute("data-src") ||
        img.getAttribute("data-original") ||
        img.getAttribute("data-lazy-src");
      const w = img.naturalWidth || img.width || 0;
      const h = img.naturalHeight || img.height || 0;
      push(url, { kind: "image", w, h, name: guessName(url || "", false) });
    }

    for (const source of document.querySelectorAll("picture source[srcset]")) {
      const best = largestSrcset(source.getAttribute("srcset"));
      if (best) push(best, { kind: "image", w: 0, h: 0, name: guessName(best, false) });
    }

    for (const video of document.querySelectorAll("video")) {
      const url = video.currentSrc || video.src || video.querySelector("source")?.src;
      const w = video.videoWidth || video.width || 0;
      const h = video.videoHeight || video.height || 0;
      const poster = video.poster ? absUrl(video.poster) : null;
      push(url, { kind: "video", w, h, poster, name: guessName(url || "", true) });
    }

    // 背景图：只扫「内联样式里带 background」的元素，比对全文档做 getComputedStyle 快几个数量级
    for (const el of document.querySelectorAll('[style*="background"]')) {
      const bg = getComputedStyle(el).backgroundImage;
      if (!bg || bg === "none") continue;
      const m = /url\(["']?([^"')]+)["']?\)/.exec(bg);
      if (m) {
        const r = el.getBoundingClientRect();
        push(m[1], {
          kind: "image",
          w: Math.round(r.width),
          h: Math.round(r.height),
          name: guessName(m[1], false),
        });
      }
    }

    // 指向图片文件的链接（论坛 / 图床列表页常见：缩略图是 <a> 套 <img>）
    for (const a of document.querySelectorAll("a[href]")) {
      const href = a.getAttribute("href");
      if (href && MEDIA_RE.test(href)) {
        push(href, { kind: "image", w: 0, h: 0, name: guessName(href, false) });
      }
    }

    const list = [...map.values()].filter((it) => {
      if (it.kind === "video" && !config.includeVideo) return false;
      if (!it.w || !it.h) return true; // 尺寸未知（懒加载）先留着
      return Math.max(it.w, it.h) >= config.minSize;
    });

    // 视频排后面，图片优先展示
    list.sort((a, b) => (a.kind === b.kind ? 0 : a.kind === "image" ? -1 : 1));
    return list;
  }

  // ───────────────────────── UI ─────────────────────────

  const host = document.createElement("div");
  host.id = "pixvault-host";
  host.style.cssText =
    "all:initial;position:fixed;top:0;left:0;width:0;height:0;z-index:2147483647;";
  const shadow = host.attachShadow({ mode: "open" });
  document.documentElement.appendChild(host);

  const style = document.createElement("style");
  style.textContent = `
  :host { all: initial; }
  * { box-sizing: border-box; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif; }
  .panel {
    position: fixed; top: 16px; right: 16px; width: 372px; max-height: calc(100vh - 32px);
    display: flex; flex-direction: column; border-radius: 14px; overflow: hidden;
    background: rgba(24,24,26,0.88); backdrop-filter: blur(22px) saturate(1.6);
    border: 1px solid rgba(255,255,255,0.12);
    box-shadow: 0 18px 50px rgba(0,0,0,0.45), inset 0 1px 0 rgba(255,255,255,0.08);
    color: #f2f2f4;
  }
  .head { display:flex; align-items:center; gap:8px; padding:11px 13px; border-bottom:1px solid rgba(255,255,255,0.08); }
  .logo { width:16px; height:16px; border-radius:5px; background:linear-gradient(135deg,#fff,#b9b9bf); flex:0 0 auto; }
  .title { font-size:13px; font-weight:600; letter-spacing:.02em; }
  .count { font-size:11px; color:rgba(255,255,255,0.5); }
  .spacer { flex:1; }
  .x { width:22px; height:22px; border:0; border-radius:6px; background:transparent; color:rgba(255,255,255,0.6);
       font-size:16px; line-height:1; cursor:pointer; }
  .x:hover { background:rgba(255,255,255,0.12); color:#fff; }
  .bar { display:flex; align-items:center; gap:6px; padding:8px 13px; }
  .chip { border:1px solid rgba(255,255,255,0.14); background:transparent; color:rgba(255,255,255,0.78);
          font-size:11px; padding:3px 9px; border-radius:999px; cursor:pointer; }
  .chip:hover { background:rgba(255,255,255,0.1); color:#fff; }
  .hint { margin-left:auto; font-size:11px; color:rgba(255,255,255,0.5); }
  .hint b { color:#ffb066; font-variant-numeric: tabular-nums; }
  .grid { overflow-y:auto; padding:4px 10px 10px; display:grid; grid-template-columns:repeat(3,1fr); gap:6px; }
  .grid::-webkit-scrollbar { width:8px; }
  .grid::-webkit-scrollbar-thumb { background:rgba(255,255,255,0.16); border-radius:4px; }
  .card { position:relative; aspect-ratio:1/1; border-radius:8px; overflow:hidden; cursor:pointer;
          background:rgba(255,255,255,0.06); border:1px solid transparent; }
  .card img { width:100%; height:100%; object-fit:cover; display:block; }
  .card .noimg { width:100%; height:100%; display:flex; align-items:center; justify-content:center;
                 font-size:10px; color:rgba(255,255,255,0.4); text-transform:uppercase; }
  .card .pick { position:absolute; top:5px; right:5px; width:16px; height:16px; border-radius:50%;
                border:1.5px solid rgba(255,255,255,0.7); background:rgba(0,0,0,0.35); }
  .card.sel { border-color:#ffb066; }
  .card.sel .pick { background:#ffb066; border-color:#ffb066; }
  .card.sel .pick::after { content:""; position:absolute; left:4.5px; top:1.5px; width:4px; height:8px;
                           border:solid #1a1a1a; border-width:0 1.8px 1.8px 0; transform:rotate(45deg); }
  .card .tag { position:absolute; left:4px; bottom:4px; font-size:9px; padding:1px 4px; border-radius:3px;
               background:rgba(0,0,0,0.6); color:#ddd; }
  .card .vplay { position:absolute; inset:0; display:flex; align-items:center; justify-content:center; }
  .card .vplay::after { content:""; width:0; height:0; border-left:11px solid rgba(255,255,255,0.9);
                        border-top:7px solid transparent; border-bottom:7px solid transparent; }
  .empty { padding:26px 12px; text-align:center; font-size:12px; color:rgba(255,255,255,0.45); }
  .foot { padding:10px 13px 12px; border-top:1px solid rgba(255,255,255,0.08); display:flex; gap:8px; align-items:center; }
  .save { flex:1; height:32px; border:0; border-radius:9px; cursor:pointer; font-size:12.5px; font-weight:600;
          background:linear-gradient(180deg,#fff,#d9d9de); color:#141416; }
  .save:disabled { opacity:.45; cursor:default; }
  .save:not(:disabled):hover { filter:brightness(1.05); }
  .foot .status { font-size:11px; color:rgba(255,255,255,0.55); min-width:0; }
  .hover {
    position: fixed; width:26px; height:26px; border-radius:8px; display:none; align-items:center; justify-content:center;
    background:rgba(24,24,26,0.86); backdrop-filter:blur(10px); border:1px solid rgba(255,255,255,0.18);
    box-shadow:0 6px 16px rgba(0,0,0,0.35); cursor:pointer; pointer-events:auto;
  }
  .hover::after { content:""; width:11px; height:11px; border-radius:3px; background:linear-gradient(135deg,#fff,#b9b9bf); }
  .hover:hover { border-color:#ffb066; }
  .toast { position: fixed; bottom: 24px; left: 50%; transform: translateX(-50%);
           background: rgba(24,24,26,0.92); backdrop-filter: blur(18px); color:#f2f2f4;
           border:1px solid rgba(255,255,255,0.12); border-radius:10px; padding:9px 14px;
           font-size:12px; box-shadow:0 12px 30px rgba(0,0,0,0.4); max-width: 70vw; }
  `;
  shadow.appendChild(style);

  const hoverBtn = document.createElement("div");
  hoverBtn.className = "hover";
  hoverBtn.title = "保存到 PixVault";
  shadow.appendChild(hoverBtn);

  let hoverUrl = null;

  function showToast(text, ms = 2600) {
    const el = document.createElement("div");
    el.className = "toast";
    el.textContent = text;
    shadow.appendChild(el);
    setTimeout(() => el.remove(), ms);
  }

  function el(tag, cls, text) {
    const n = document.createElement(tag);
    if (cls) n.className = cls;
    if (text != null) n.textContent = text;
    return n;
  }

  function buildPanel() {
    const panel = el("div", "panel");

    const head = el("div", "head");
    head.appendChild(el("div", "logo"));
    head.appendChild(el("div", "title", "PixVault"));
    const count = el("div", "count", "");
    head.appendChild(count);
    head.appendChild(el("div", "spacer"));
    const close = el("button", "x", "×");
    close.addEventListener("click", () => togglePanel(false));
    head.appendChild(close);
    panel.appendChild(head);

    const bar = el("div", "bar");
    const mk = (label, fn) => {
      const b = el("button", "chip", label);
      b.addEventListener("click", fn);
      return b;
    };
    bar.appendChild(mk("全选", () => { selected = new Set(media.map((m) => m.url)); renderGrid(); }));
    bar.appendChild(mk("清空", () => { selected.clear(); renderGrid(); }));
    bar.appendChild(mk("反选", () => {
      const next = new Set();
      for (const m of media) if (!selected.has(m.url)) next.add(m.url);
      selected = next;
      renderGrid();
    }));
    const hint = el("div", "hint");
    bar.appendChild(hint);
    panel.appendChild(bar);

    const grid = el("div", "grid");
    panel.appendChild(grid);

    const foot = el("div", "foot");
    const save = el("button", "save", "保存到 PixVault");
    save.addEventListener("click", doSave);
    const status = el("div", "status", "");
    foot.appendChild(save);
    foot.appendChild(status);
    panel.appendChild(foot);

    panel.__parts = { count, grid, hint, save, status };
    return panel;
  }

  let panel = null;

  function togglePanel(force) {
    const open = force === undefined ? !panelOpen : force;
    if (open === panelOpen) return;
    panelOpen = open;
    if (open && !panel) {
      panel = buildPanel();
      shadow.appendChild(panel);
      rescan();
    } else if (panel) {
      panel.style.display = open ? "flex" : "none";
      if (open) rescan();
    }
    if (!open) hideHover();
  }

  function rescan() {
    media = collectMedia();
    // 丢掉已经不在页面上的选中项
    const alive = new Set(media.map((m) => m.url));
    selected = new Set([...selected].filter((u) => alive.has(u)));
    renderGrid();
  }

  function renderGrid() {
    if (!panel) return;
    const { count, grid, hint, status } = panel.__parts;
    count.textContent = `${media.length} 项`;
    hint.innerHTML = "";
    hint.append("已选 ");
    hint.appendChild(el("b", null, String(selected.size)));

    grid.innerHTML = "";
    if (media.length === 0) {
      grid.appendChild(el("div", "empty", "这个页面上没有找到可摘取的图片或视频"));
    }

    for (const item of media) {
      const card = el("div", "card");
      if (selected.has(item.url)) card.classList.add("sel");

      if (item.kind === "video") {
        if (item.poster) {
          const img = document.createElement("img");
          img.src = item.poster;
          img.loading = "lazy";
          card.appendChild(img);
        }
        const vplay = el("div", "vplay");
        card.appendChild(vplay);
        card.appendChild(el("div", "tag", "视频"));
      } else {
        const img = document.createElement("img");
        img.src = item.url;
        img.loading = "lazy";
        img.decoding = "async";
        img.addEventListener("error", () => {
          img.remove();
          card.insertBefore(el("div", "noimg", item.name.split(".").pop() || "img"), card.firstChild);
        });
        card.appendChild(img);
        if (item.w && item.h) {
          card.appendChild(el("div", "tag", `${Math.round(item.w)}×${Math.round(item.h)}`));
        }
      }

      card.appendChild(el("div", "pick"));
      card.title = item.name;
      card.addEventListener("click", () => {
        if (selected.has(item.url)) selected.delete(item.url);
        else selected.add(item.url);
        card.classList.toggle("sel");
        hint.innerHTML = "";
        hint.append("已选 ");
        hint.appendChild(el("b", null, String(selected.size)));
        updateSaveBtn();
      });
      grid.appendChild(card);
    }
    updateSaveBtn();
    if (!busy) status.textContent = "";
  }

  function updateSaveBtn() {
    if (!panel) return;
    const { save } = panel.__parts;
    save.disabled = busy || selected.size === 0;
    save.textContent = busy ? "保存中…" : `保存到 PixVault（${selected.size}）`;
  }

  async function doSave() {
    if (busy || selected.size === 0) return;
    busy = true;
    updateSaveBtn();
    const items = media.filter((m) => selected.has(m.url)).map((m) => ({
      url: m.url,
      name: m.name,
    }));

    let resp;
    try {
      resp = await chrome.runtime.sendMessage({
        type: "pixvault:save",
        items,
        pageUrl: location.href,
      });
    } catch (e) {
      resp = { started: false, error: String(e) };
    }

    if (!resp || !resp.started) {
      busy = false;
      updateSaveBtn();
      showToast(`保存失败：${(resp && resp.error) || "后台没有响应"}`);
      return;
    }
    if (panel) panel.__parts.status.textContent = `0/${items.length}`;
  }

  function onProgress(msg) {
    if (!panel || !busy) return;
    panel.__parts.status.textContent = `${msg.current}/${msg.total}`;
  }

  function onDone(msg) {
    busy = false;
    if (panel) {
      updateSaveBtn();
      panel.__parts.status.textContent = "";
    }
    if (msg.fail === 0) {
      showToast(`已保存 ${msg.ok} 项到 PixVault`);
      if (panel) {
        selected.clear();
        renderGrid();
      }
    } else {
      showToast(`保存完成：成功 ${msg.ok} 项，失败 ${msg.fail} 项`, 5000);
      console.warn("[PixVault] 失败明细", msg.errors);
    }
  }

  // ───────────────────────── 悬停采集按钮 ─────────────────────────

  function hideHover() {
    hoverUrl = null;
    hoverBtn.style.display = "none";
  }

  function nearestMedia(node) {
    let cur = node;
    let depth = 0;
    while (cur && cur.nodeType === 1 && depth < 4) {
      if (cur.tagName === "IMG" || cur.tagName === "VIDEO") return cur;
      cur = cur.parentElement;
      depth += 1;
    }
    return null;
  }

  document.addEventListener(
    "mouseover",
    (e) => {
      if (!config.hoverButton) {
        hideHover();
        return;
      }
      if (e.target === host || (e.composedPath && e.composedPath().includes(host))) return;

      const mediaEl = nearestMedia(e.target);
      if (!mediaEl) {
        hideHover();
        return;
      }
      const rect = mediaEl.getBoundingClientRect();
      if (rect.width < 48 || rect.height < 48) {
        hideHover();
        return;
      }

      const isVideo = mediaEl.tagName === "VIDEO";
      const srcset = mediaEl.getAttribute && mediaEl.getAttribute("srcset");
      const url =
        mediaEl.currentSrc ||
        mediaEl.src ||
        (srcset && largestSrcset(srcset)) ||
        mediaEl.getAttribute("data-src");
      const abs = absUrl(url);
      if (!abs) {
        hideHover();
        return;
      }

      hoverUrl = { url: abs, name: guessName(abs, isVideo) };
      hoverBtn.style.display = "flex";
      hoverBtn.style.top = `${Math.min(rect.top + 6, window.innerHeight - 34)}px`;
      hoverBtn.style.left = `${Math.min(rect.left + 6, window.innerWidth - 34)}px`;
    },
    true,
  );

  hoverBtn.addEventListener("mousedown", (e) => {
    // 别让点击穿透到页面（很多图片外面套着 <a>，点一下会跳走）
    e.preventDefault();
    e.stopPropagation();
  });

  hoverBtn.addEventListener("click", async (e) => {
    e.preventDefault();
    e.stopPropagation();
    if (!hoverUrl) return;
    const item = { ...hoverUrl };
    hideHover();
    showToast(`正在保存 ${item.name} …`, 1600);
    try {
      await chrome.runtime.sendMessage({
        type: "pixvault:save",
        items: [item],
        pageUrl: location.href,
      });
    } catch (err) {
      showToast(`保存失败：${String(err)}`);
    }
  });

  window.addEventListener("scroll", hideHover, { passive: true, capture: true });
  window.addEventListener("resize", hideHover, { passive: true });

  window.addEventListener(
    "keydown",
    (e) => {
      if (e.altKey && !e.ctrlKey && !e.metaKey && (e.key === "s" || e.key === "S")) {
        e.preventDefault();
        e.stopPropagation();
        togglePanel();
      }
      if (e.key === "Escape" && panelOpen) {
        togglePanel(false);
      }
    },
    true,
  );

  // ───────────────────────── 消息 ─────────────────────────

  function bufferToBase64(buf) {
    const bytes = new Uint8Array(buf);
    let binary = "";
    const chunk = 0x8000;
    for (let i = 0; i < bytes.length; i += chunk) {
      binary += String.fromCharCode.apply(null, bytes.subarray(i, i + chunk));
    }
    return btoa(binary);
  }

  chrome.runtime.onMessage.addListener((msg, sender, sendResponse) => {
    if (!msg || typeof msg.type !== "string") return;

    if (msg.type === "pixvault:grab") {
      (async () => {
        try {
          const res = await fetch(msg.url);
          const blob = await res.blob();
          const buf = await blob.arrayBuffer();
          sendResponse({ ok: true, base64: bufferToBase64(buf), mime: blob.type });
        } catch (e) {
          sendResponse({ ok: false, error: String(e) });
        }
      })();
      return true;
    }

    if (msg.type === "pixvault:toggle-panel") {
      togglePanel(true);
      sendResponse({ open: true, found: media.length });
      return true;
    }

    if (msg.type === "pixvault:progress") {
      onProgress(msg);
      return;
    }
    if (msg.type === "pixvault:done") {
      onDone(msg);
      return;
    }
    if (msg.type === "pixvault:config-changed") {
      chrome.storage.local.get({ minSize: 64, hoverButton: true, includeVideo: true }).then((c) => {
        config = c;
      });
      return;
    }
  });

  chrome.storage.local.get({ minSize: 64, hoverButton: true, includeVideo: true }).then((c) => {
    config = c;
  });
})();
