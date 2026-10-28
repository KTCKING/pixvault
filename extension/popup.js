const dot = document.getElementById("dot");
const statusEl = document.getElementById("status");
const libEl = document.getElementById("lib");
const openBtn = document.getElementById("open");

function setState(kind, text, detail) {
  dot.className = `dot ${kind}`;
  statusEl.textContent = text;
  libEl.textContent = detail || "";
}

async function refresh() {
  setState("", "正在连接 PixVault…", "");
  let res;
  try {
    res = await chrome.runtime.sendMessage({ type: "pixvault:ping" });
  } catch (e) {
    res = { ok: false, error: String(e) };
  }

  if (res && res.ok && res.library) {
    setState("ok", "已连接 PixVault", `${res.library.name} · ${res.library.root_path}`);
    openBtn.disabled = false;
  } else if (res && res.ok && !res.library) {
    setState("bad", "PixVault 里没有打开的图库", "在应用里打开一个图库后再试");
    openBtn.disabled = false;
  } else {
    const err = (res && res.error) || "未知错误";
    const hint = /token|401/i.test(err)
      ? "访问令牌不匹配：到扩展设置里填入 PixVault 设置面板中的令牌"
      : /Failed to fetch|NetworkError|拒绝/i.test(err)
        ? "确认 PixVault 正在运行，且设置里「浏览器扩展」开关是打开的"
        : err;
    setState("bad", "连不上 PixVault", hint);
    openBtn.disabled = false;
  }
}

/** 新装扩展时，已经打开的页面里没有 content script —— 先注入再发消息。 */
async function ensureContentScript(tabId) {
  try {
    await chrome.tabs.sendMessage(tabId, { type: "pixvault:toggle-panel" });
  } catch {
    await chrome.scripting.executeScript({ target: { tabId }, files: ["content.js"] });
    await chrome.tabs.sendMessage(tabId, { type: "pixvault:toggle-panel" });
  }
}

openBtn.addEventListener("click", async () => {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  if (!tab || !tab.id) return;
  try {
    await ensureContentScript(tab.id);
    window.close();
  } catch (e) {
    setState("bad", "这个页面上打不开面板", String(e));
  }
});

document.getElementById("settings").addEventListener("click", () => {
  chrome.runtime.openOptionsPage();
  window.close();
});

void refresh();
