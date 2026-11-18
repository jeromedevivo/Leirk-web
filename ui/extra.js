"use strict";
/* Side panels, developer mode, hover cards, sync settings. Injected after the main chrome script. */

/* ---------- side panels ---------- */
const PRESETS = [
  ["Teams", "https://teams.microsoft.com/"], ["Spotify", "https://open.spotify.com/"], ["WhatsApp", "https://web.whatsapp.com/"],
  ["Discord", "https://discord.com/app"], ["Outlook", "https://outlook.live.com/mail/"], ["Gmail", "https://mail.google.com/"],
  ["Messenger", "https://www.messenger.com/"], ["Telegram", "https://web.telegram.org/"], ["Slack", "https://app.slack.com/client"],
  ["Notion", "https://www.notion.so/"], ["ChatGPT", "https://chatgpt.com/"], ["Claude", "https://claude.ai/"],
  ["Google Agenda", "https://calendar.google.com/"], ["Deezer", "https://www.deezer.com/"], ["YouTube Music", "https://music.youtube.com/"], ["X", "https://x.com/"],
];
let sidePanel = null; // "dev" | panel id
let appIcons = {};
const panels = () => P("panels") || [];
const devOn = () => !!P("dev_mode");
const mod = m => devOn() && (P("dev_modules") || {})[m] !== false;

window.sideRect = function () {
  const side = $("#side");
  if (side.hidden || sidePanel === "dev") return { sx: 0, sy: 0, sw: 0, sh: 0 };
  const r = $("#sidebody").getBoundingClientRect();
  return { sx: r.left, sy: r.top, sw: r.width, sh: r.height };
};

function panelIcon(p) {
  if (p.icon) return `<span class="fav img pimg"><img src="${esc(p.icon)}" alt=""></span>`;
  if (p.kind === "app") return `<span class="pico app">${esc((p.title || "?").charAt(0).toUpperCase())}</span>`;
  return fav({ url: p.url, title: p.title });
}
function renderPanelBar() {
  const bar = $("#panelbar");
  bar.hidden = P("panel_bar") === false && !panels().length;
  bar.innerHTML = panels().map(p => `<button class="pbtn${sidePanel === p.id ? " on" : ""}" data-panel="${esc(p.id)}" title="${esc(p.title)}${p.kind === "app" ? " · application Windows (expérimental)" : ""}">${panelIcon(p)}</button>`).join("")
    + `<button class="pbtn add" id="panelAdd" title="Ajouter un panneau">${icon("plus")}</button>`;
  const side = $("#side");
  side.hidden = !sidePanel;
  side.style.width = (P("panel_width") || 420) + "px";
  if (sidePanel) {
    const p = panels().find(x => x.id === sidePanel);
    $("#sidetitle").textContent = sidePanel === "dev" ? "Mode développeur" : (p?.title || "");
    $("#sideReload").hidden = sidePanel === "dev";
    $("#sideUnload").hidden = sidePanel === "dev";
    $("#sidebody").classList.toggle("dev", sidePanel === "dev");
    $("#sidebody").innerHTML = "";
  }
  sendLayout();
}
function openPanel(id) {
  if (sidePanel === id) { sidePanel = null; send("panelClose"); }
  else { sidePanel = id; $("#sidebody").innerHTML = ""; send("panelOpen", id); }
  renderPanelBar();
}
$("#panelbar").addEventListener("click", e => {
  const b = e.target.closest(".pbtn"); if (!b) return;
  if (b.id === "panelAdd") return openPanelAdd();
  openPanel(b.dataset.panel);
});
$("#panelbar").addEventListener("contextmenu", e => {
  const b = e.target.closest(".pbtn[data-panel]"); if (!b || b.dataset.panel === "dev") return;
  e.preventDefault();
  const id = b.dataset.panel;
  showMenu("#ctx", [
    ["reload", "Recharger", () => { if (sidePanel !== id) openPanel(id); send("panelReload"); }],
    ["moon", "Fermer et libérer la mémoire", () => { if (sidePanel === id) sidePanel = null; send("panelUnload", id); renderPanelBar(); }],
    "-",
    ["trash", "Retirer le panneau", () => removePanel(id)],
  ], e.clientX - 280, e.clientY);
});
function removePanel(id) { send("panelUnload", id); if (sidePanel === id) sidePanel = null; setPref("panels", panels().filter(p => p.id !== id)); renderPanelBar(); }
$("#sideClose").onclick = () => { sidePanel = null; send("panelClose"); renderPanelBar(); };
$("#sideReload").onclick = () => send("panelReload");
$("#sideUnload").onclick = () => { const id = sidePanel; sidePanel = null; send("panelUnload", id); renderPanelBar(); };
$("#sidegrip").addEventListener("mousedown", e => {
  e.preventDefault();
  const x0 = e.clientX, w0 = $("#side").offsetWidth;
  const move = ev => { $("#side").style.width = Math.min(900, Math.max(280, w0 + x0 - ev.clientX)) + "px"; sendLayout(); };
  const up = () => { removeEventListener("mousemove", move); removeEventListener("mouseup", up); setPref("panel_width", $("#side").offsetWidth); };
  addEventListener("mousemove", move); addEventListener("mouseup", up);
});

function openPanelAdd() {
  const box = $("#panelPop");
  box.innerHTML = `<h2>Ajouter un panneau</h2>
    <div class="presets">${PRESETS.map(([n, u]) => `<button class="preset" data-name="${esc(n)}" data-url="${esc(u)}">${fav({ url: u, title: n })}<span>${esc(n)}</span></button>`).join("")}</div>
    <label class="lbl">Autre site</label><div class="inline"><input class="in" id="panelUrl" placeholder="https://…"><button class="btn" id="panelUrlAdd">Ajouter</button></div>
    <label class="lbl">Application Windows ouverte <span class="tag">expérimental</span></label>
    <small class="muted">La fenêtre de l'application est intégrée au panneau. Elle reprend sa place normale si vous la retirez ou quittez Leirk.</small>
    <div id="appList" class="applist"><small class="muted">Recherche des applications…</small></div>`;
  show("panelPop", "#panelAdd");
  const p = $("#panelPop"); p.style.left = Math.max(8, innerWidth - p.offsetWidth - 60) + "px"; p.style.top = "90px";
  send("panelApps");
}
function addPanel(p) {
  p.id = "p" + Date.now().toString(36);
  setPref("panels", [...panels(), p]);
  closeAll();
  setTimeout(() => openPanel(p.id), 50);
}
$("#panelPop").addEventListener("click", e => {
  const pre = e.target.closest(".preset");
  if (pre) return addPanel({ kind: "web", title: pre.dataset.name, url: pre.dataset.url });
  if (e.target.id === "panelUrlAdd") {
    let u = $("#panelUrl").value.trim(); if (!u) return;
    if (!/^https?:/.test(u)) u = "https://" + u;
    return addPanel({ kind: "web", title: hostOf(u), url: u });
  }
  const app = e.target.closest("[data-exe]");
  if (app) addPanel({ kind: "app", title: app.dataset.title, exe: app.dataset.exe, icon: appIcons[app.dataset.exe] || "" });
});
L.panelApps = list => {
  const seen = new Set();
  const uniq = list.filter(a => !seen.has(a.exe.toLowerCase()) && seen.add(a.exe.toLowerCase()));
  const box = $("#appList"); if (!box) return;
  appIcons = Object.fromEntries(uniq.map(a => [a.exe, a.icon]));
  box.innerHTML = uniq.length ? uniq.map(a => { const n = a.exe.split("\\").pop(); return `<button class="row" data-exe="${esc(a.exe)}" data-title="${esc(n.replace(/\.exe$/i, ""))}">${a.icon ? `<span class="fav img pimg"><img src="${esc(a.icon)}" alt=""></span>` : `<span class="pico app">${esc(n.charAt(0).toUpperCase())}</span>`}<span class="t">${esc(a.title)}</span><span class="k">${esc(n)}</span></button>`; }).join("")
    : `<small class="muted">Aucune application ouverte. Lancez Teams ou Spotify puis rouvrez ce menu.</small>`;
};

/* ---------- hover card (Brave): preview + memory ---------- */
let hoverTimer = 0, hoverId = 0;
function hideCard() {
  clearTimeout(hoverTimer); hoverId = 0;
  if (openPop === "hovercard") closeAll(); else $("#hovercard").classList.remove("on");
}
for (const bar of [$("#tabs"), $("#vlist"), $("#stackbar")]) {
  bar.addEventListener("mouseover", e => {
    const el = e.target.closest(".tab"); if (!el || P("hover_card") === "off") return;
    const id = +el.dataset.id; if (id === hoverId) return;
    clearTimeout(hoverTimer);
    hoverTimer = setTimeout(() => showCard(id, el), $("#hovercard").classList.contains("on") ? 60 : 450);
  });
  bar.addEventListener("mouseleave", hideCard);
  bar.addEventListener("mousedown", hideCard);
}
function showCard(id, el) {
  const t = S.tabs.find(x => x.id === id); if (!t || (openPop && openPop !== "hovercard")) return;
  hoverId = id;
  const card = $("#hovercard");
  const preview = P("hover_card") !== "title" && t.url;
  card.innerHTML = `${preview ? `<div class="thumb" id="hcThumb">${t.loaded ? "" : `<span>${icon("moon")}Déchargé de la mémoire</span>`}</div>` : ""}
    <div class="hc"><b>${esc(tabTitle(t))}</b><span class="muted">${esc(hostOf(t.url) || "Nouvel onglet")}${t.container ? ` · conteneur ${esc(t.container)}` : ""}</span>
    <span class="mem" id="hcMem">${t.loaded ? "Mémoire : …" : t.url ? "Mémoire : 0 Mo · se recharge au clic" : "Mémoire : 0 Mo"}</span>
    ${t.errors ? `<span class="hcerr">${t.errors} erreur${t.errors > 1 ? "s" : ""} JavaScript</span>` : ""}</div>`;
  const r = el.getBoundingClientRect(), vertical = P("tabs_position") === "left";
  card.style.left = Math.max(8, Math.min(innerWidth - 300, vertical ? r.right + 8 : r.left)) + "px";
  card.style.top = (vertical ? r.top : r.bottom + 6) + "px";
  // shown like a menu: the page's snapshot slides under the chrome so the card can float over it
  show("hovercard");
  send("tabInfo", id);
}

/* Developer tools live in the DevTools "Leirk" tab (F12); the chrome only shows hover cards and captures here. */
L.dev = (kind, data) => {
  if (kind === "tabInfo") {
    if (data.id !== hoverId) return;
    const mem = $("#hcMem"); if (mem && data.mb != null && S.tabs.find(t => t.id === data.id)?.loaded) mem.textContent = `Mémoire : ${data.mb} Mo`;
    const th = $("#hcThumb");
    if (th && data.thumb) th.innerHTML = `<img src="${data.thumb}" alt="">`;
    else if (th && S.tabs.find(t => t.id === data.id)?.loaded) th.remove(); // on screen right now: nothing to preview
    return;
  }
  if (kind === "capture") return data.ok ? toast(`Capture enregistrée (${data.w}×${data.h} px) dans Images › Leirk`) : toast(`Capture impossible : ${data.error}`);
};

/* Developer tools, shared by the panel and F2 ("jwt …", "b64 …"). */
function devTool(kind, v) {
  const b64e = s => btoa(String.fromCharCode(...new TextEncoder().encode(s)));
  const b64d = s => new TextDecoder().decode(Uint8Array.from(atob(s.replace(/-/g, "+").replace(/_/g, "/").padEnd(Math.ceil(s.length / 4) * 4, "=")), c => c.charCodeAt(0)));
  try {
    switch (kind) {
      case "jwt": { const [h, p] = v.split("."); const pl = JSON.parse(b64d(p)); const exp = pl.exp ? `\n\nexpire : ${new Date(pl.exp * 1000).toLocaleString("fr-FR")}${pl.exp * 1000 < Date.now() ? " (expiré)" : ""}` : ""; return JSON.stringify(JSON.parse(b64d(h)), null, 2) + "\n" + JSON.stringify(pl, null, 2) + exp; }
      case "b64": return b64e(v);
      case "d64": return b64d(v);
      case "url": return encodeURIComponent(v);
      case "durl": return decodeURIComponent(v);
      case "json": return JSON.stringify(JSON.parse(v), null, 2);
      case "ts": { if (/^\d{9,13}$/.test(v)) { const n = +v; const d = new Date(v.length > 10 ? n : n * 1000); return `${d.toLocaleString("fr-FR")}\n${d.toISOString()}`; } const d = v ? new Date(v) : new Date(); return isNaN(d) ? null : `${Math.floor(d / 1000)}\n${+d} ms`; }
      case "uuid": return crypto.randomUUID();
      case "len": return `${[...v].length} caractères · ${new TextEncoder().encode(v).length} octets · ${v.split(/\s+/).filter(Boolean).length} mots · ${v.split("\n").length} lignes`;
    }
  } catch { return null; }
  return null;
}

/* ---------- palette additions ---------- */
window.extraGroups = function (q, groups) {
  if (!q || !mod("tools")) return;
  const m = q.match(/^(jwt|b64|d64|url|durl|json|ts|uuid|len)\b\s*([\s\S]*)$/i);
  if (!m) return;
  const out = devTool(m[1].toLowerCase(), m[2].trim());
  if (out != null) groups.push(["Outil développeur", [{ icon: "cmd", t: out.split("\n")[0].slice(0, 120), u: out.includes("\n") ? "…" : "", k: "Copier", act: () => { navigator.clipboard?.writeText(out); toast("Copié"); } }]]);
};
window.lateGroups = function (q, groups) {
  const remote = (S.remoteTabs || []).filter(t => q && match(q, t.title, t.url, t.device)).slice(0, 6)
    .map(t => ({ fav: t, t: t.title || t.url, u: hostOf(t.url), k: t.device, act: nt => send(nt ? "open" : "nav", t.url) }));
  if (remote.length) groups.push(["Onglets de mes autres appareils", remote]);
};
COMMANDS.push(
  ["download", "Capture de la page entière", () => send("capture"), "Ctrl+Maj+S", () => !!S.url],
  ["code", "Mode développeur : activer / désactiver", () => setPref("dev_mode", !devOn())],
  ["code", "Outils développeur Leirk (DevTools)", () => send("devtools"), "F12", () => devOn() && !!S.url],
  ["code", "Couper / réactiver JavaScript sur ce site", () => send("noscript"), "", () => !!S.shields.site],
  ["tabs", "Ajouter un panneau latéral", () => openPanelAdd()],
);
const baseCommandsLen = COMMANDS.length;
function refreshCommands() {
  COMMANDS.length = baseCommandsLen;
  for (const p of panels()) COMMANDS.push(["tabs", `Panneau : ${p.title}`, () => openPanel(p.id)]);
  if (mod("containers")) for (const n of (P("dev_containers") || "").split("\n").map(s => s.trim()).filter(Boolean))
    COMMANDS.push(["stack", `Ouvrir dans le conteneur « ${n} »`, () => send("openContainer", { name: n }), "", () => !!S.url]);
}

/* ---------- tab extras: JS errors dot, container colour, context menu ---------- */
const ctColor = n => `oklch(0.62 0.16 ${[...n].reduce((a, c) => a + c.charCodeAt(0) * 37, 0) % 360})`;
window.tabExtra = t => ({
  cls: (t.container ? " ct" : ""),
  style: t.container ? `--ct:${ctColor(t.container)}` : "",
  html: t.errors && mod("errors") ? `<span class="errdot" title="${t.errors} erreur(s) JavaScript"></span>` : "",
});
window.tabMenuExtra = id => {
  const names = (P("dev_containers") || "").split("\n").map(s => s.trim()).filter(Boolean);
  const t = S.tabs.find(x => x.id === id);
  return mod("containers") && t?.url ? ["-", ...names.map(n => ["stack", `Rouvrir dans le conteneur « ${n} »`, () => send("openContainer", { name: n, url: t.url })])] : [];
};

/* ---------- per-state refresh ---------- */
window.renderExtras = function () {
  if (sidePanel && sidePanel !== "dev" && !panels().some(p => p.id === sidePanel)) sidePanel = null;
  refreshCommands();
  renderPanelBar();
};

/* ---------- settings sections ---------- */
SECTIONS.splice(SECTIONS.findIndex(s => s.id === "import"), 0,
  { id: "panels", icon: "tabs", title: "Panneaux latéraux", items: [
    { k: "panel_bar", t: "Afficher la barre de panneaux", d: "La colonne d'icônes à droite de la fenêtre.", type: "toggle" },
    { t: "Ajouter un panneau", d: "Teams, Spotify, WhatsApp, Discord… ou une application Windows ouverte (expérimental).", type: "button", label: "Ajouter", act: () => { closeAll(); setTimeout(openPanelAdd, 50); } },
    { type: "panels" },
  ]},
  { id: "sync", icon: "reload", title: "Synchronisation", items: [{ type: "sync" }] },
  { id: "dev", icon: "code", title: "Mode développeur", items: [
    { k: "dev_mode", t: "Activer le mode développeur", d: "Ajoute l'onglet « Leirk » aux DevTools (F12) avec les modules choisis. Désactivé, l'onglet n'affiche rien et rien ne tourne en arrière-plan.", type: "toggle" },
    { h: "Modules" },
    { k: "dev_modules.js", t: "JavaScript en un clic", d: "Bouton JS dans l'onglet Leirk des DevTools pour couper ou réactiver les scripts du site.", type: "sub" },
    { k: "dev_modules.tech", t: "Technologies (Wappalyzer)", d: "Frameworks, CMS, serveurs, analytics… avec leurs versions.", type: "sub" },
    { k: "dev_modules.errors", t: "Pastille d'erreurs JavaScript", d: "Point rouge sur l'onglet et nombre d'erreurs dans l'aperçu au survol. Le détail est dans la Console.", type: "sub" },
    { k: "dev_modules.docker", t: "Docker", d: "Projets Compose puis leurs conteneurs : tout démarrer ou arrêter d'un clic, ports cliquables, CPU et RAM, logs.", type: "sub" },
    { k: "dev_modules.ports", t: "Ports locaux", d: "Serveurs web en écoute sur ce PC (vite, node, IIS…), ouvrables d'un clic.", type: "sub" },
    { k: "dev_modules.tools", t: "Outils", d: "Décoder un JWT, Base64, encodage d'URL, JSON, timestamp, identifiant unique, changement d'environnement — aussi dans F2.", type: "sub" },
    { k: "dev_modules.containers", t: "Conteneurs de session", d: "Onglets aux cookies isolés : connectez-vous en dev et en prod en même temps.", type: "sub" },
    { h: "Configuration" },
    { k: "dev_envs", t: "Environnements", d: "Un hôte par ligne, par ex. localhost:3000, staging.monapp.fr, monapp.fr.", type: "textarea" },
    { k: "dev_containers", t: "Conteneurs", d: "Un nom par ligne.", type: "textarea" },
  ]},
);
SECTIONS.find(s => s.id === "tabs").items.push({ k: "hover_card", t: "Carte au survol d'un onglet", d: "Aperçu de la page et mémoire utilisée, comme dans Brave.", type: "select", o: [["preview", "Aperçu et mémoire"], ["title", "Titre et mémoire"], ["off", "Désactivée"]] });
let syncProviders = null;
L.syncProviders = list => { syncProviders = list; if (openPop === "settings") renderSettings(false); };
window.customItem = function (it) {
  if (it.type === "panels") {
    const list = panels();
    return `<div class="set col">${list.length ? list.map(p => `<div class="drow">${panelIcon(p)}<span class="t">${esc(p.title)}</span><span class="k muted">${p.kind === "app" ? "application · expérimental" : esc(hostOf(p.url))}</span><button class="icon" data-panel-del="${esc(p.id)}" aria-label="Retirer ${esc(p.title)}">${icon("trash")}</button></div>`).join("") : `<small>Aucun panneau pour l'instant.</small>`}</div>`;
  }
  if (it.type === "sync") {
    if (!syncProviders) send("syncProviders");
    const cur = P("sync_folder") || "";
    const last = S.lastSync ? new Date(S.lastSync * 1000).toLocaleTimeString("fr-FR") : "jamais";
    return `<div class="set col"><div><div>Synchronisez favoris, réglages, moteurs de recherche et onglets ouverts entre vos PC.</div>
      <small>Leirk écrit un fichier par appareil dans un dossier de votre cloud (OneDrive, Google Drive, Dropbox, iCloud…), que le logiciel du cloud synchronise. Aucun serveur, aucun compte Leirk.</small></div>
      <div class="chips">${(syncProviders || []).map(p => `<button class="chipbtn${cur === p.path ? " on" : ""}" data-sync="${esc(p.path)}">${esc(p.name)}</button>`).join("") || `<small class="muted">${syncProviders ? "Aucun dossier cloud détecté." : "Recherche…"}</small>`}
      ${cur ? `<button class="chipbtn" data-sync="">Désactiver</button>` : ""}</div>
      <div class="inline"><input class="in" id="syncPath" placeholder="Ou un autre dossier : D:\\Nextcloud" value="${esc(cur)}"><button class="btn" id="syncSet">Utiliser</button></div>
      ${cur ? `<div class="drow"><span class="t">Cet appareil : <b>${esc(S.device)}</b> · dernière synchro ${last}</span><button class="btn" id="syncNow">Synchroniser</button></div>
      ${(S.syncDevices || []).map(d => `<div class="drow"><span class="t">${esc(d.device)}</span><span class="k muted">${new Date(d.updated * 1000).toLocaleString("fr-FR")}</span></div>`).join("") || `<small class="muted">Aucun autre appareil pour l'instant : installez Leirk sur un autre PC et choisissez le même dossier.</small>`}` : ""}</div>`;
  }
  return "";
};
$("#setbody").addEventListener("click", e => {
  const del = e.target.closest("[data-panel-del]");
  if (del) { removePanel(del.dataset.panelDel); return setTimeout(() => renderSettings(false), 100); }
  const s = e.target.closest("[data-sync]");
  if (s) { setPref("sync_folder", s.dataset.sync); toast(s.dataset.sync ? "Synchronisation activée" : "Synchronisation désactivée"); return setTimeout(() => renderSettings(false), 400); }
  if (e.target.id === "syncSet") { setPref("sync_folder", $("#syncPath").value.trim()); return setTimeout(() => renderSettings(false), 400); }
  if (e.target.id === "syncNow") return send("syncNow");
});

renderExtras();
