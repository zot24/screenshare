const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

let isSharing = false;
let viewerInterval = null;

async function init() {
  const [hostname, ip] = await invoke("get_identity");
  document.getElementById("identity").textContent = `You: ${hostname} (${ip})`;

  await refreshWindows();
  setInterval(refreshWindows, 2000);

  await listen("sharers-updated", (event) => {
    renderSharers(event.payload);
  });

  document.getElementById("share-btn").addEventListener("click", toggleShare);
  document.getElementById("back-btn").addEventListener("click", stopViewing);
}

async function refreshWindows() {
  if (isSharing) return;
  const windows = await invoke("list_windows");
  const container = document.getElementById("window-list");
  container.innerHTML = "";
  for (const win of windows) {
    const label = win.title
      ? `${win.app_name} - ${win.title.length > 60 ? win.title.slice(0, 57) + "..." : win.title}`
      : win.app_name;
    const div = document.createElement("label");
    div.innerHTML = `<input type="radio" name="source" value="window"
      data-id="${win.id}" data-app="${escapeHtml(win.app_name)}"
      data-title="${escapeHtml(win.title)}" /> ${escapeHtml(label)}`;
    container.appendChild(div);
  }
}

async function toggleShare() {
  if (isSharing) {
    await invoke("stop_sharing");
    isSharing = false;
    document.getElementById("share-btn").textContent = "Share";
    document.getElementById("share-status").textContent = "";
    document.getElementById("source-picker").disabled = false;
  } else {
    const selected = document.querySelector('input[name="source"]:checked');
    if (!selected) return;

    let args;
    if (selected.value === "fullscreen") {
      args = { sourceType: "fullscreen" };
    } else {
      args = {
        sourceType: "window",
        windowId: parseInt(selected.dataset.id),
        windowApp: selected.dataset.app,
        windowTitle: selected.dataset.title,
      };
    }

    try {
      await invoke("start_sharing", args);
      isSharing = true;
      document.getElementById("share-btn").textContent = "Stop Sharing";
      const label =
        selected.value === "fullscreen"
          ? "Full Screen"
          : selected.dataset.app;
      document.getElementById("share-status").textContent = `Sharing: ${label}`;
      document.getElementById("source-picker").disabled = true;
    } catch (e) {
      alert("Failed to start sharing: " + e);
    }
  }
}

function renderSharers(sharers) {
  const container = document.getElementById("sharers-list");
  if (sharers.length === 0) {
    container.innerHTML = '<p class="muted">Scanning local network...</p>';
    return;
  }
  container.innerHTML = "";
  for (const s of sharers) {
    const div = document.createElement("div");
    div.className = "sharer-row";
    const sourceTag = s.source === "tailscale" ? " [tailscale]" : "";
    const sharingLabel = s.sharing ? ` \u2014 ${s.sharing}` : "";
    div.innerHTML = `<span>${escapeHtml(s.hostname)} (${escapeHtml(s.ip)})${sharingLabel}${sourceTag}</span>
      <button class="view-btn">View</button>`;
    div.querySelector(".view-btn").addEventListener("click", () => {
      startViewing(s.hostname, s.ip, s.port);
    });
    container.appendChild(div);
  }
}

async function startViewing(name, ip, port) {
  try {
    await invoke("start_viewing", { ip, port });
  } catch (e) {
    alert("Failed to connect: " + e);
    return;
  }

  document.getElementById("home-screen").style.display = "none";
  document.getElementById("viewer-screen").style.display = "block";
  document.getElementById("viewer-label").textContent = `Viewing: ${name}`;

  const img = document.getElementById("viewer-img");
  const connecting = document.getElementById("viewer-connecting");
  img.style.display = "none";
  connecting.style.display = "block";

  viewerInterval = setInterval(async () => {
    const dataUrl = await invoke("poll_frame");
    if (dataUrl) {
      img.src = dataUrl;
      img.style.display = "block";
      connecting.style.display = "none";
    }
  }, 67);
}

async function stopViewing() {
  if (viewerInterval) {
    clearInterval(viewerInterval);
    viewerInterval = null;
  }
  await invoke("stop_viewing");
  document.getElementById("viewer-screen").style.display = "none";
  document.getElementById("home-screen").style.display = "block";
}

function escapeHtml(str) {
  const div = document.createElement("div");
  div.textContent = str;
  return div.innerHTML;
}

init();
