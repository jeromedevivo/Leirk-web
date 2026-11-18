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
  bar.hidden = P("panel_bar") === false && !devOn() && !panels().length;
  bar.innerHTML = (devOn() ? `<button class="pbtn${sidePanel === "dev" ? " on" : ""}" data-panel="dev" title="Mode développeur">${icon("code")}</button><hr>` : "")
    + panels().map(p => `<button class="pbtn${sidePanel === p.id ? " on" : ""}" data-panel="${esc(p.id)}" title="${esc(p.title)}${p.kind === "app" ? " · application Windows (expérimental)" : ""}">${panelIcon(p)}</button>`).join("")
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
    if (sidePanel === "dev") { if (!$("#devbody")) renderDev(); }
    else $("#sidebody").innerHTML = "";
  } else clearInterval(devTimer);
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

/* ---------- developer panel ---------- */
const DEV_MODULES = [
  ["tech", "palette", "Technologies"], ["network", "globe", "Réseau"], ["errors", "x", "Erreurs JS"], ["docker", "stack", "Docker"],
  ["ports", "tabs", "Ports locaux"], ["perf", "cpu", "Performances"], ["storage", "key", "Cookies et stockage"], ["responsive", "tile", "Responsive"], ["tools", "cmd", "Outils"],
];
let devTab = "tech", devData = {}, devTimer = 0, netFilter = "", logsFor = "", devUrl = "";
function devModules() { return DEV_MODULES.filter(([m]) => mod(m)); }
function renderDev() {
  const mods = devModules();
  if (!mods.length) { $("#sidebody").innerHTML = `<p class="muted pad">Aucun module actif. Choisissez-les dans Réglages › Mode développeur.</p>`; return; }
  if (!mods.some(([m]) => m === devTab)) devTab = mods[0][0];
  $("#sidebody").innerHTML = `<nav class="devnav">${mods.map(([m, i, t]) => `<button data-dev="${m}" class="${m === devTab ? "on" : ""}" title="${t}">${icon(i)}<span>${t}</span></button>`).join("")}</nav><div id="devbody" class="devbody"></div>`;
  devRefresh();
}
function devRefresh() {
  clearInterval(devTimer);
  const ask = { tech: "devTech", network: "devNet", errors: "devErrors", docker: "devDocker", ports: "devPorts", perf: "devPerf", storage: "devCookies" }[devTab];
  if (ask) { send(ask); if (devTab === "storage") send("devStorage"); }
  const every = { network: 1000, errors: 1500, docker: 5000, perf: 2000 }[devTab];
  if (every) devTimer = setInterval(() => { if (sidePanel === "dev" && !document.hidden) send(ask); }, every);
  devUrl = S.url;
  devPaint();
}
$("#sidebody").addEventListener("click", e => {
  const nav = e.target.closest("[data-dev]");
  if (nav) { devTab = nav.dataset.dev; renderDev(); return; }
  const a = e.target.closest("[data-act]"); if (!a) return;
  devAction(a.dataset.act, a.dataset);
});
$("#sidebody").addEventListener("input", e => { if (e.target.id === "netFilter") { netFilter = e.target.value; devPaint(true); } if (e.target.id === "toolIn") toolRun(); });
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
  devData[kind] = data;
  if (sidePanel === "dev") devPaint();
};
const fmtBytes = b => b == null ? "—" : b < 1024 ? b + " o" : b < 1048576 ? (b / 1024).toFixed(1) + " Ko" : (b / 1048576).toFixed(1) + " Mo";
function devPaint(keepInput) {
  const body = $("#devbody"); if (!body) return;
  if (document.activeElement && body.contains(document.activeElement) && /INPUT|TEXTAREA/.test(document.activeElement.tagName) && !keepInput) return;
  const d = devData, keepScroll = body.scrollTop;
  let html = "";
  switch (devTab) {
    case "tech": {
      const t = d.tech;
      if (!S.url) html = `<p class="muted">Ouvrez un site pour voir ses technologies.</p>`;
      else if (!t || t.loading) html = `<p class="muted">${t?.loading ? "Téléchargement de la base Wappalyzer (une seule fois)…" : "Analyse…"}</p>`;
      else {
        const groups = {}; (t.found || []).forEach(f => (groups[f.category || "Autres"] ||= []).push(f));
        html = `<div class="devbar"><b>${(t.found || []).length} technologies</b><button class="btn" data-act="techRefresh">Analyser à nouveau</button></div>`
          + (Object.keys(groups).length ? Object.entries(groups).map(([c, l]) => `<h4>${esc(c)}</h4>` + l.map(f =>
            `<div class="drow"><img class="ticon" src="https://raw.githubusercontent.com/enthec/webappanalyzer/main/src/images/icons/${encodeURIComponent(f.icon)}" alt="" onerror="this.style.visibility='hidden'"><span class="t">${esc(f.name)}</span><span class="k">${esc(f.version)}</span></div>`).join("")).join("")
          : `<p class="muted">Aucune technologie reconnue parmi ${t.known}.</p>`);
      }
      break;
    }
    case "network": {
      const all = d.network || [];
      const rows = all.filter(r => !netFilter || r.u.toLowerCase().includes(netFilter.toLowerCase())).slice(-250).reverse();
      html = `<div class="devbar"><input class="in" id="netFilter" placeholder="Filtrer les adresses" value="${esc(netFilter)}"><button class="btn" data-act="netClear">Effacer</button></div>
        <div class="muted small">${all.length} requêtes · ${all.filter(r => r.b).length} bloquées par Shields · rechargez la page pour tout voir</div>
        <table class="net"><thead><tr><th>Statut</th><th>Type</th><th>Adresse</th><th>Taille</th><th>ms</th><th></th></tr></thead><tbody>${rows.map(r => `<tr class="${r.b ? "blocked" : r.s >= 400 ? "bad" : ""}">
          <td>${r.b ? "bloqué" : r.s || "…"}</td><td>${esc(r.t)}</td><td class="u" title="${esc(r.u)}">${esc(r.m)} ${esc(r.u.replace(/^https?:\/\//, ""))}</td><td>${fmtBytes(r.z)}</td><td>${r.d ?? ""}</td>
          <td class="acts"><button data-act="copyUrl" data-u="${esc(r.u)}" title="Copier l'adresse">${icon("tabs")}</button><button data-act="copyCurl" data-u="${esc(r.u)}" data-m="${esc(r.m)}" title="Copier en cURL">${icon("cmd")}</button><button data-act="replay" data-u="${esc(r.u)}" data-m="${esc(r.m)}" title="Rejouer">${icon("reload")}</button></td></tr>`).join("")}</tbody></table>`;
      break;
    }
    case "errors": {
      const errs = d.errors || [];
      html = `<div class="devbar"><b>${errs.length} erreur${errs.length > 1 ? "s" : ""}</b><button class="btn" data-act="errClear">Effacer</button></div>`
        + (errs.length ? errs.slice().reverse().map(e => `<div class="errrow"><div>${esc(e.msg)}</div><small class="muted">${esc(e.src)}</small></div>`).join("") : `<p class="muted">Aucune erreur sur cette page. Une pastille rouge apparaît sur l'onglet dès qu'il y en a.</p>`);
      break;
    }
    case "docker": {
      const k = d.docker;
      if (!k) html = `<p class="muted">Lecture de Docker…</p>`;
      else if (k.error) html = `<p class="muted">Docker ne répond pas : ${esc(k.error)}</p>`;
      else html = `<div class="devbar"><b>${k.containers.length} conteneurs · ${k.containers.filter(c => c.state === "running").length} actifs</b><button class="btn" data-act="dockerRefresh">Actualiser</button></div>`
        + k.containers.map(c => `<div class="card"><div class="ctop"><span class="dot ${c.state === "running" ? "ok" : ""}"></span><b>${esc(c.name)}</b><span class="muted small">${esc(c.image)}</span></div>
          <div class="muted small">${esc(c.status)}${c.cpu ? ` · CPU ${esc(c.cpu)} · RAM ${esc(c.mem)}` : ""}</div>
          <div class="cact">${c.ports.map(p => `<button class="chipbtn" data-act="openPort" data-port="${p}">localhost:${p}</button>`).join("")}
          <span class="grow"></span>${c.state === "running" ? `<button class="btn" data-act="docker" data-id="${c.id}" data-op="restart">Redémarrer</button><button class="btn" data-act="docker" data-id="${c.id}" data-op="stop">Arrêter</button>` : `<button class="btn primary" data-act="docker" data-id="${c.id}" data-op="start">Démarrer</button>`}
          <button class="btn" data-act="logs" data-id="${c.id}">Logs</button></div>${logsFor === c.id && d.logs?.id === c.id ? `<pre class="logs">${esc(d.logs.text)}</pre>` : ""}</div>`).join("");
      break;
    }
    case "ports": {
      const p = d.ports || [];
      html = `<div class="devbar"><b>${p.length} ports en écoute</b><button class="btn" data-act="portsRefresh">Actualiser</button></div>`
        + p.map(x => `<div class="drow"><b class="port">${x.port}</b><span class="t">${esc(x.process || "?")}</span><span class="k muted">pid ${x.pid}</span><button class="btn" data-act="openPort" data-port="${x.port}">Ouvrir</button></div>`).join("");
      break;
    }
    case "perf": {
      const a = d.perfApp || {}, m = d.perfMetrics || {}, v = d.vitals || {};
      const grade = (val, good, bad) => val == null ? "" : val <= good ? "good" : val <= bad ? "mid" : "bad";
      html = `<h4>Page active</h4><div class="kpis">
        <div class="kpi ${grade(v.lcp, 2500, 4000)}"><b>${v.lcp ?? "—"}</b><span>LCP ms</span></div><div class="kpi ${grade(v.cls, 0.1, 0.25)}"><b>${v.cls ?? "—"}</b><span>CLS</span></div>
        <div class="kpi ${grade(v.fcp, 1800, 3000)}"><b>${v.fcp ?? "—"}</b><span>FCP ms</span></div><div class="kpi ${grade(v.ttfb, 800, 1800)}"><b>${v.ttfb ?? "—"}</b><span>TTFB ms</span></div></div>
        <div class="drow"><span class="t">Tas JavaScript</span><span class="k">${fmtBytes(m.JSHeapUsedSize)}</span></div><div class="drow"><span class="t">Nœuds DOM</span><span class="k">${v.nodes ?? m.Nodes ?? "—"}</span></div>
        <div class="drow"><span class="t">Mises en page</span><span class="k">${m.LayoutCount ?? "—"}</span></div><div class="drow"><span class="t">Transféré</span><span class="k">${fmtBytes(v.transfer)}</span></div><div class="drow"><span class="t">Chargement complet</span><span class="k">${v.load ?? "—"} ms</span></div>
        <h4>Leirk</h4><div class="drow"><span class="t">Processus principal</span><span class="k">${a.mb ?? "—"} Mo · CPU ${a.cpu ?? 0} %</span></div>
        <h4>Onglets en mémoire</h4>${(d.perfTabs || []).map(t => `<div class="drow"><span class="t">${esc(t.title || "")}</span><span class="k">${t.mb} Mo</span></div>`).join("") || `<p class="muted">—</p>`}`;
      break;
    }
    case "storage": {
      const ck = d.cookies || [], st = d.storage || { local: [], session: [] };
      const kv = (area, list) => list.map(([k, v]) => `<div class="drow"><span class="t" title="${esc(v)}">${esc(k)}</span><span class="k mono">${esc(String(v).slice(0, 40))}</span><button class="icon" data-act="stEdit" data-area="${area}" data-k="${esc(k)}" title="Modifier">${icon("edit")}</button><button class="icon" data-act="stDel" data-area="${area}" data-k="${esc(k)}" title="Supprimer">${icon("trash")}</button></div>`).join("") || `<p class="muted small">Vide.</p>`;
      html = `<div class="devbar"><b>${ck.length} cookies</b><button class="btn" data-act="storageRefresh">Actualiser</button></div>`
        + ck.map(c => `<div class="drow"><span class="t" title="${esc(c.domain + c.path)}">${esc(c.name)}</span><span class="k mono">${esc(String(c.value).slice(0, 40))}</span><button class="icon" data-act="ckEdit" data-name="${esc(c.name)}" data-domain="${esc(c.domain)}" data-path="${esc(c.path)}" title="Modifier">${icon("edit")}</button><button class="icon" data-act="ckDel" data-name="${esc(c.name)}" data-domain="${esc(c.domain)}" data-path="${esc(c.path)}" title="Supprimer">${icon("trash")}</button></div>`).join("")
        + `<h4>localStorage</h4>${kv("local", st.local || [])}<h4>sessionStorage</h4>${kv("session", st.session || [])}
        <div class="inline" style="margin-top:8px"><input class="in" id="stK" placeholder="clé"><input class="in" id="stV" placeholder="valeur"><button class="btn" data-act="stAdd">Ajouter</button></div>`;
      break;
    }
    case "responsive": {
      const envs = (P("dev_envs") || "").split("\n").map(s => s.trim()).filter(Boolean);
      const dev = [["iPhone 15", 393, 852, 3, 1], ["Pixel 8", 412, 915, 2.6, 1], ["iPad", 820, 1180, 2, 1], ["Portable", 1366, 768, 1, 0], ["Full HD", 1920, 1080, 1, 0]];
      html = `<h4>Appareil</h4><div class="chips">${dev.map(([n, w, h, r, m]) => `<button class="chipbtn" data-act="device" data-w="${w}" data-h="${h}" data-r="${r}" data-m="${m}">${n} · ${w}×${h}</button>`).join("")}<button class="chipbtn" data-act="deviceReset">Taille réelle</button></div>
        <div class="inline" style="margin-top:8px"><input class="in" id="devW" placeholder="largeur" type="number"><input class="in" id="devH" placeholder="hauteur" type="number"><button class="btn" data-act="deviceCustom">Appliquer</button></div>
        <h4>Réseau</h4><div class="chips">${[["normal", "Normal"], ["4g", "4G"], ["3g", "3G lente"], ["offline", "Hors ligne"]].map(([v, l]) => `<button class="chipbtn" data-act="throttle" data-v="${v}">${l}</button>`).join("")}</div>
        <h4>Environnements</h4>${envs.length ? `<div class="chips">${envs.map(h => `<button class="chipbtn" data-act="env" data-host="${esc(h)}">${esc(h)}</button>`).join("")}</div><small class="muted">Ouvre la même page sur cet hôte.</small>` : `<p class="muted small">Ajoutez vos hôtes (localhost:3000, staging.monapp.fr, monapp.fr) dans Réglages › Mode développeur.</p>`}`;
      break;
    }
    case "tools":
      if ($("#toolIn")) return;
      html = `<textarea class="in" id="toolIn" rows="5" placeholder="Collez un JWT, du Base64, du JSON, un timestamp, une URL…"></textarea>
        <div class="chips" style="margin:8px 0">${[["jwt", "Décoder JWT"], ["b64", "→ Base64"], ["d64", "Base64 →"], ["url", "→ URL"], ["durl", "URL →"], ["json", "Formater JSON"], ["ts", "Timestamp ↔ date"], ["uuid", "UUID"], ["len", "Compter"]].map(([k, l]) => `<button class="chipbtn" data-act="tool" data-k="${k}">${l}</button>`).join("")}</div>
        <pre class="logs" id="toolOut">Ces outils marchent aussi dans F2 : « jwt eyJ… », « b64 texte », « ts 1700000000 », « uuid ».</pre>`;
      break;
  }
  const focus = document.activeElement?.id;
  body.innerHTML = html;
  body.scrollTop = keepScroll;
  if (focus === "netFilter") { const f = $("#netFilter"); f.focus(); f.setSelectionRange(f.value.length, f.value.length); }
}
let toolKind = "jwt";
function toolRun(k) {
  if (k) toolKind = k;
  const v = $("#toolIn")?.value || "", out = devTool(toolKind, v.trim());
  if ($("#toolOut")) $("#toolOut").textContent = out ?? "Entrée non reconnue.";
}
function devAction(act, ds) {
  const curl = (m, u) => `curl -X ${m} '${u.replace(/'/g, "'\\''")}' -H 'User-Agent: ${navigator.userAgent}'`;
  switch (act) {
    case "techRefresh": devData.tech = null; devPaint(); return send("devTech");
    case "netClear": send("devNetClear"); devData.network = []; return devPaint();
    case "copyUrl": navigator.clipboard?.writeText(ds.u); return toast("Adresse copiée");
    case "copyCurl": navigator.clipboard?.writeText(curl(ds.m, ds.u)); return toast("Commande cURL copiée");
    case "replay": send("devReplay", { u: ds.u, m: ds.m }); return toast("Requête rejouée depuis la page");
    case "errClear": send("devErrorsClear"); devData.errors = []; return devPaint();
    case "dockerRefresh": return send("devDocker");
    case "docker": toast(`Docker : ${ds.op}…`); return send("devDockerAct", { id: ds.id, act: ds.op });
    case "logs": logsFor = logsFor === ds.id ? "" : ds.id; if (logsFor) send("devDockerLogs", ds.id); return devPaint();
    case "openPort": return send("open", `http://localhost:${ds.port}`);
    case "portsRefresh": return send("devPorts");
    case "storageRefresh": send("devCookies"); return send("devStorage");
    case "ckDel": send("devCookieDel", { name: ds.name, domain: ds.domain, path: ds.path }); return setTimeout(() => send("devCookies"), 150);
    case "ckEdit": { const val = prompt(`Nouvelle valeur de ${ds.name}`); if (val == null) return; send("devCookieSet", { name: ds.name, value: val, domain: ds.domain, path: ds.path }); return setTimeout(() => send("devCookies"), 150); }
    case "stDel": send("devStorageDel", { area: ds.area, k: ds.k }); return setTimeout(() => send("devStorage"), 150);
    case "stEdit": { const val = prompt(`Nouvelle valeur de ${ds.k}`); if (val == null) return; send("devStorageSet", { area: ds.area, k: ds.k, v: val }); return setTimeout(() => send("devStorage"), 150); }
    case "stAdd": { const k = $("#stK").value.trim(); if (!k) return; send("devStorageSet", { area: "local", k, v: $("#stV").value }); return setTimeout(() => send("devStorage"), 150); }
    case "device": send("devDevice", { w: +ds.w, h: +ds.h, dpr: +ds.r, mobile: ds.m === "1" }); return toast(`Émulation ${ds.w}×${ds.h}`);
    case "deviceCustom": { const w = +$("#devW").value, h = +$("#devH").value; if (w && h) send("devDevice", { w, h, dpr: 1, mobile: false }); return; }
    case "deviceReset": send("devDevice", null); return toast("Taille réelle");
    case "throttle": send("devThrottle", ds.v); return toast(ds.v === "normal" ? "Réseau normal" : `Réseau simulé : ${ds.v}`);
    case "env": {
      try { const u = new URL(S.url); const local = /^localhost|^127\.|^\d+\.\d+\.\d+\.\d+/.test(ds.host); send("nav", `${local ? "http" : "https"}://${ds.host}${u.pathname}${u.search}${u.hash}`); } catch { send("nav", ds.host); }
      return;
    }
    case "tool": return toolRun(ds.k);
  }
}

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
  ["code", "Ouvrir le panneau développeur", () => { if (sidePanel !== "dev") openPanel("dev"); }, "", () => devOn()],
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

/* ---------- JS toggle button and per-state refresh ---------- */
$("#jsbtn").onclick = () => { toast(S.shields.noscript ? "JavaScript réactivé sur ce site" : "JavaScript coupé sur ce site"); send("noscript"); };
window.renderExtras = function () {
  const b = $("#jsbtn");
  b.hidden = !(mod("js") && S.url);
  b.classList.toggle("off", !!S.shields.noscript);
  b.title = S.shields.noscript ? "JavaScript coupé sur ce site — cliquer pour réactiver" : "Couper JavaScript sur ce site";
  if (!devOn() && sidePanel === "dev") sidePanel = null;
  if (sidePanel && sidePanel !== "dev" && !panels().some(p => p.id === sidePanel)) sidePanel = null;
  if (sidePanel === "dev" && S.url !== devUrl && $("#devbody")) devRefresh();
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
    { k: "dev_mode", t: "Activer le mode développeur", d: "Ajoute le panneau </> et les modules choisis. Désactivé, rien ne tourne en arrière-plan.", type: "toggle" },
    { h: "Modules" },
    { k: "dev_modules.js", t: "JavaScript en un clic", d: "Bouton JS dans la barre d'outils pour couper ou réactiver les scripts du site.", type: "sub" },
    { k: "dev_modules.tech", t: "Technologies (Wappalyzer)", d: "Frameworks, CMS, serveurs, analytics… avec leurs versions.", type: "sub" },
    { k: "dev_modules.network", t: "Journal réseau", d: "Requêtes de l'onglet, statuts, tailles ; copier en cURL et rejouer.", type: "sub" },
    { k: "dev_modules.errors", t: "Erreurs JavaScript", d: "Pastille rouge sur l'onglet et liste des exceptions.", type: "sub" },
    { k: "dev_modules.docker", t: "Docker", d: "Conteneurs, ports cliquables, CPU et RAM, logs, démarrer et arrêter.", type: "sub" },
    { k: "dev_modules.ports", t: "Ports locaux", d: "Serveurs en écoute sur ce PC (vite, node, postgres…).", type: "sub" },
    { k: "dev_modules.perf", t: "Performances", d: "RAM par onglet, Web Vitals, tas JS, nœuds DOM.", type: "sub" },
    { k: "dev_modules.storage", t: "Cookies et stockage", d: "Voir, modifier et supprimer cookies, localStorage et sessionStorage.", type: "sub" },
    { k: "dev_modules.responsive", t: "Responsive et réseau", d: "Tailles d'appareils, 3G/4G/hors ligne, changement d'environnement.", type: "sub" },
    { k: "dev_modules.tools", t: "Outils", d: "JWT, Base64, URL, JSON, timestamp, UUID — aussi dans F2.", type: "sub" },
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
