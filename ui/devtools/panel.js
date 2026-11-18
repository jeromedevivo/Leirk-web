"use strict";
/* Leirk's developer tools, inside the DevTools "Leirk" tab. Talks to Leirk over its token-guarded loopback API. */
if (chrome.devtools.panels.themeName === "dark") document.documentElement.classList.add("dark");
const $ = s => document.querySelector(s);
const esc = s => String(s ?? "").replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
const fmtBytes = b => b == null ? "—" : b < 1024 ? b + " o" : b < 1048576 ? (b / 1024).toFixed(1) + " Ko" : (b / 1048576).toFixed(1) + " Mo";

// Which tab this DevTools window inspects: the page tells Leirk its tab, keyed by a nonce only we know.
const nonce = crypto.randomUUID();
const hello = () => chrome.devtools.inspectedWindow.eval(`window.ipc && window.ipc.postMessage("devhello:${nonce}")`);
hello();
chrome.devtools.network.onNavigated.addListener(() => { hello(); data = {}; setTimeout(refresh, 600); });

const MODULES = [["tech", "Technologies"], ["network", "Réseau"], ["errors", "Erreurs JS"], ["docker", "Docker"], ["ports", "Ports locaux"], ["perf", "Performances"], ["storage", "Cookies et stockage"], ["tools", "Outils"]];
const ASK = { tech: "devTech", network: "devNet", errors: "devErrors", docker: "devDocker", ports: "devPorts", perf: "devPerf", storage: "devCookies" };
const EVERY = { network: 1000, errors: 1500, docker: 5000, perf: 2000 };
let state = { dev: false, modules: {} }, data = {}, tab = "tech", filter = "", logsFor = "", lastAsk = 0;

async function api(c, v) {
  try {
    const r = await fetch(`http://127.0.0.1:${LEIRK.port}/`, { method: "POST", headers: { "content-type": "application/json", "x-leirk-token": LEIRK.token }, body: JSON.stringify({ c, v, tab: nonce }) });
    const j = await r.json();
    for (const [kind, d] of j.outbox || []) data[kind] = d;
    state = j;
    return j;
  } catch { state = { offline: true }; }
}
const mods = () => MODULES.filter(([m]) => state.modules?.[m] !== false);
function refresh() { const c = ASK[tab]; if (c) { api(c); if (tab === "storage") api("devStorage"); } lastAsk = Date.now(); }

async function loop() {
  await api("poll");
  if (EVERY[tab] && Date.now() - lastAsk > EVERY[tab]) refresh();
  render();
  setTimeout(loop, 700);
}

function render() {
  const js = $("#js");
  js.hidden = !state.dev || state.modules?.js === false;
  js.classList.toggle("off", !!state.noscript);
  if (state.offline) return void ($("#body").innerHTML = `<p class="muted">Leirk ne répond pas. Fermez puis rouvrez les DevTools.</p>`);
  if (!state.dev) { $("#nav").innerHTML = ""; return void ($("#body").innerHTML = `<p class="muted">Le mode développeur est désactivé. Activez-le dans Leirk › Réglages › Mode développeur.</p>`); }
  const m = mods();
  if (!m.some(([k]) => k === tab) && m.length) { tab = m[0][0]; refresh(); }
  const nav = m.map(([k, l]) => `<button data-tab="${k}" class="${k === tab ? "on" : ""}">${l}</button>`).join("");
  if ($("#nav").innerHTML !== nav) $("#nav").innerHTML = nav;
  const body = $("#body");
  if (body.contains(document.activeElement) && /INPUT|TEXTAREA/.test(document.activeElement.tagName) && document.activeElement.id !== "filter") return;
  const html = paint(), scroll = body.scrollTop, focus = document.activeElement?.id;
  if (html === body.dataset.last) return;
  body.dataset.last = html; body.innerHTML = html; body.scrollTop = scroll;
  if (focus === "filter") { const f = $("#filter"); f.focus(); f.setSelectionRange(f.value.length, f.value.length); }
}

function paint() {
  const d = data;
  switch (tab) {
    case "tech": {
      const t = d.tech;
      if (!t || t.loading) return `<p class="muted">${t?.loading ? "Téléchargement de la base Wappalyzer (une seule fois)…" : "Analyse…"}</p>`;
      const g = {}; (t.found || []).forEach(f => (g[f.category || "Autres"] ||= []).push(f));
      return `<div class="bar"><b>${(t.found || []).length} technologies</b><span class="grow"></span><button class="btn" data-act="tech">Analyser à nouveau</button></div>`
        + (Object.keys(g).length ? Object.entries(g).map(([c, l]) => `<h4>${esc(c)}</h4>` + l.map(f => `<div class="row"><img class="ic" alt="" src="https://raw.githubusercontent.com/enthec/webappanalyzer/main/src/images/icons/${encodeURIComponent(f.icon)}"><span class="t">${esc(f.name)}</span><span class="k">${esc(f.version)}</span></div>`).join("")).join("") : `<p class="muted">Aucune technologie reconnue parmi ${esc(t.known)}.</p>`);
    }
    case "network": {
      const all = d.network || [], rows = all.filter(r => !filter || r.u.toLowerCase().includes(filter.toLowerCase())).slice(-300).reverse();
      return `<div class="bar"><input id="filter" placeholder="Filtrer les adresses" value="${esc(filter)}"><button class="btn" data-act="netClear">Effacer</button></div>
        <p class="muted">${all.length} requêtes · ${all.filter(r => r.b).length} bloquées par Shields (marquées 403 dans la console)</p>
        <table><thead><tr><th style="width:56px">Statut</th><th style="width:90px">Type</th><th>Adresse</th><th style="width:64px">Taille</th><th style="width:44px">ms</th><th style="width:150px"></th></tr></thead><tbody>${rows.map(r => `<tr class="${r.b ? "blocked" : r.s >= 400 ? "bad" : ""}"><td>${r.b ? "bloqué" : esc(r.s || "…")}</td><td>${esc(r.t)}</td><td title="${esc(r.u)}">${esc(r.m)} ${esc(r.u)}</td><td>${fmtBytes(r.z)}</td><td>${esc(r.d ?? "")}</td>
          <td><button class="btn" data-act="copyUrl" data-u="${esc(r.u)}">URL</button><button class="btn" data-act="curl" data-u="${esc(r.u)}" data-m="${esc(r.m)}">cURL</button><button class="btn" data-act="replay" data-u="${esc(r.u)}" data-m="${esc(r.m)}">Rejouer</button></td></tr>`).join("")}</tbody></table>`;
    }
    case "errors": {
      const e = d.errors || [];
      return `<div class="bar"><b>${e.length} erreur${e.length > 1 ? "s" : ""} sur cette page</b><span class="grow"></span><button class="btn" data-act="errClear">Effacer</button></div>`
        + (e.length ? e.slice().reverse().map(x => `<div class="err">${esc(x.msg)}<div class="muted">${esc(x.src)}</div></div>`).join("") : `<p class="muted">Aucune erreur. La liste repart de zéro à chaque nouvelle page.</p>`);
    }
    case "docker": {
      const k = d.docker;
      if (!k) return `<p class="muted">Lecture de Docker…</p>`;
      if (k.error) return `<p class="muted">Docker ne répond pas : ${esc(k.error)}</p>`;
      return `<div class="bar"><b>${k.containers.length} conteneurs · ${k.containers.filter(c => c.state === "running").length} actifs</b></div>` + k.containers.map(c => `<div class="card">
        <div class="bar"><span class="dot ${c.state === "running" ? "ok" : ""}"></span><b>${esc(c.name)}</b><span class="muted">${esc(c.image)}</span></div>
        <div class="muted">${esc(c.status)}${c.cpu ? ` · CPU ${esc(c.cpu)} · RAM ${esc(c.mem)}` : ""}</div>
        <div class="chips">${c.ports.map(p => `<button class="btn" data-act="port" data-port="${esc(p)}">localhost:${esc(p)}</button>`).join("")}<span class="grow"></span>
        ${c.state === "running" ? `<button class="btn" data-act="docker" data-id="${esc(c.id)}" data-op="restart">Redémarrer</button><button class="btn" data-act="docker" data-id="${esc(c.id)}" data-op="stop">Arrêter</button>` : `<button class="btn prim" data-act="docker" data-id="${esc(c.id)}" data-op="start">Démarrer</button>`}
        <button class="btn" data-act="logs" data-id="${esc(c.id)}">Logs</button></div>${logsFor === c.id && d.logs?.id === c.id ? `<pre>${esc(d.logs.text)}</pre>` : ""}</div>`).join("");
    }
    case "ports": {
      const p = d.ports || [];
      return `<div class="bar"><b>${p.length} ports en écoute</b><span class="grow"></span><button class="btn" data-act="ports">Actualiser</button></div>` + p.map(x => `<div class="row"><b style="min-width:52px">${esc(x.port)}</b><span class="t">${esc(x.process || "?")}</span><span class="k">pid ${esc(x.pid)}</span><button class="btn" data-act="port" data-port="${esc(x.port)}">Ouvrir</button></div>`).join("");
    }
    case "perf": {
      const a = d.perfApp || {}, m = d.perfMetrics || {}, v = d.vitals || {};
      const g = (x, ok, bad) => x == null ? "" : x <= ok ? "good" : x <= bad ? "mid" : "bad";
      return `<h4>Web Vitals</h4><div class="kpis"><div class="kpi ${g(v.lcp, 2500, 4000)}"><b>${esc(v.lcp ?? "—")}</b>LCP ms</div><div class="kpi ${g(v.cls, .1, .25)}"><b>${esc(v.cls ?? "—")}</b>CLS</div><div class="kpi ${g(v.fcp, 1800, 3000)}"><b>${esc(v.fcp ?? "—")}</b>FCP ms</div><div class="kpi ${g(v.ttfb, 800, 1800)}"><b>${esc(v.ttfb ?? "—")}</b>TTFB ms</div></div>
        <h4>Page</h4><div class="row"><span class="t">Tas JavaScript</span><span class="k">${fmtBytes(m.JSHeapUsedSize)}</span></div><div class="row"><span class="t">Nœuds DOM</span><span class="k">${esc(v.nodes ?? m.Nodes ?? "—")}</span></div><div class="row"><span class="t">Transféré</span><span class="k">${fmtBytes(v.transfer)}</span></div><div class="row"><span class="t">Chargement complet</span><span class="k">${esc(v.load ?? "—")} ms</span></div>
        <h4>Leirk</h4><div class="row"><span class="t">Processus principal</span><span class="k">${esc(a.mb ?? "—")} Mo · CPU ${esc(a.cpu ?? 0)} %</span></div>
        <h4>Onglets en mémoire</h4>${(d.perfTabs || []).map(t => `<div class="row"><span class="t">${esc(t.title)}</span><span class="k">${esc(t.mb)} Mo</span></div>`).join("")}`;
    }
    case "storage": {
      const ck = d.cookies || [], st = d.storage || {};
      const kv = (area, l) => (l || []).map(([k, v]) => `<div class="row"><span class="t">${esc(k)}</span><span class="k mono">${esc(String(v).slice(0, 50))}</span><button class="btn" data-act="stEdit" data-area="${area}" data-k="${esc(k)}">Modifier</button><button class="btn" data-act="stDel" data-area="${area}" data-k="${esc(k)}">Supprimer</button></div>`).join("") || `<p class="muted">Vide.</p>`;
      return `<div class="bar"><b>${ck.length} cookies</b><span class="grow"></span><button class="btn" data-act="storage">Actualiser</button></div>`
        + ck.map(c => `<div class="row"><span class="t" title="${esc(c.domain + c.path)}">${esc(c.name)}</span><span class="k mono">${esc(String(c.value).slice(0, 50))}</span><button class="btn" data-act="ckEdit" data-name="${esc(c.name)}" data-domain="${esc(c.domain)}" data-path="${esc(c.path)}">Modifier</button><button class="btn" data-act="ckDel" data-name="${esc(c.name)}" data-domain="${esc(c.domain)}" data-path="${esc(c.path)}">Supprimer</button></div>`).join("")
        + `<h4>localStorage</h4>${kv("local", st.local)}<h4>sessionStorage</h4>${kv("session", st.session)}`;
    }
    case "tools": {
      const envs = (state.envs || "").split("\n").map(s => s.trim()).filter(Boolean);
      return `<h4>Environnements</h4>${envs.length ? `<div class="chips">${envs.map(h => `<button class="btn" data-act="env" data-host="${esc(h)}">${esc(h)}</button>`).join("")}</div>` : `<p class="muted">Ajoutez vos hôtes dans Leirk › Réglages › Mode développeur.</p>`}
        <h4>Responsive et réseau lent</h4><p class="muted">Utilisez la barre d'appareils des DevTools (Ctrl+Maj+M) et l'onglet Réseau › Limitation.</p>
        <h4>Convertisseurs</h4><textarea id="tin" rows="5" placeholder="Collez un JWT, du Base64, du JSON, un timestamp, une URL…"></textarea>
        <div class="chips">${[["jwt", "Décoder JWT"], ["b64", "→ Base64"], ["d64", "Base64 →"], ["url", "→ URL"], ["durl", "URL →"], ["json", "Formater JSON"], ["ts", "Timestamp ↔ date"], ["uuid", "UUID"]].map(([k, l]) => `<button class="btn" data-act="tool" data-k="${k}">${l}</button>`).join("")}</div><pre id="tout"> </pre>`;
    }
  }
  return "";
}

function tool(kind, v) {
  const b64e = s => btoa(String.fromCharCode(...new TextEncoder().encode(s)));
  const b64d = s => new TextDecoder().decode(Uint8Array.from(atob(s.replace(/-/g, "+").replace(/_/g, "/").padEnd(Math.ceil(s.length / 4) * 4, "=")), c => c.charCodeAt(0)));
  try {
    switch (kind) {
      case "jwt": { const [h, p] = v.split("."); const pl = JSON.parse(b64d(p)); return JSON.stringify(JSON.parse(b64d(h)), null, 2) + "\n" + JSON.stringify(pl, null, 2) + (pl.exp ? `\n\nexpire : ${new Date(pl.exp * 1000).toLocaleString("fr-FR")}` : ""); }
      case "b64": return b64e(v);
      case "d64": return b64d(v);
      case "url": return encodeURIComponent(v);
      case "durl": return decodeURIComponent(v);
      case "json": return JSON.stringify(JSON.parse(v), null, 2);
      case "ts": { if (/^\d{9,13}$/.test(v)) { const d = new Date(v.length > 10 ? +v : +v * 1000); return `${d.toLocaleString("fr-FR")}\n${d.toISOString()}`; } const d = v ? new Date(v) : new Date(); return isNaN(d) ? "Date non reconnue" : `${Math.floor(d / 1000)}`; }
      case "uuid": return crypto.randomUUID();
    }
  } catch { return "Entrée non reconnue."; }
}

document.addEventListener("click", e => {
  const n = e.target.closest("[data-tab]");
  if (n) { tab = n.dataset.tab; refresh(); return render(); }
  const a = e.target.closest("[data-act]"); if (!a) return;
  const ds = a.dataset;
  const later = c => setTimeout(() => api(c), 150);
  switch (ds.act) {
    case "tech": data.tech = null; return api("devTech");
    case "netClear": data.network = []; return api("devNetClear");
    case "copyUrl": return navigator.clipboard.writeText(ds.u);
    case "curl": return navigator.clipboard.writeText(`curl -X ${ds.m} '${ds.u.replace(/'/g, "'\\''")}'`);
    case "replay": return api("devReplay", { u: ds.u, m: ds.m });
    case "errClear": data.errors = []; return api("devErrorsClear");
    case "docker": return api("devDockerAct", { id: ds.id, act: ds.op });
    case "logs": logsFor = logsFor === ds.id ? "" : ds.id; if (logsFor) api("devDockerLogs", ds.id); return render();
    case "port": return api("open", `http://localhost:${ds.port}`);
    case "ports": return api("devPorts");
    case "storage": api("devCookies"); return api("devStorage");
    case "ckDel": api("devCookieDel", { name: ds.name, domain: ds.domain, path: ds.path }); return later("devCookies");
    case "ckEdit": { const v = prompt(`Nouvelle valeur de ${ds.name}`); if (v == null) return; api("devCookieSet", { name: ds.name, value: v, domain: ds.domain, path: ds.path }); return later("devCookies"); }
    case "stDel": api("devStorageDel", { area: ds.area, k: ds.k }); return later("devStorage");
    case "stEdit": { const v = prompt(`Nouvelle valeur de ${ds.k}`); if (v == null) return; api("devStorageSet", { area: ds.area, k: ds.k, v }); return later("devStorage"); }
    case "env": {
      chrome.devtools.inspectedWindow.eval("location.pathname + location.search + location.hash", path => {
        const local = /^localhost|^127\.|^\d+\.\d+\.\d+\.\d+/.test(ds.host);
        api("open", `${local ? "http" : "https"}://${ds.host}${path || "/"}`);
      });
      return;
    }
    case "tool": $("#tout").textContent = tool(ds.k, $("#tin").value.trim()) ?? ""; return;
  }
});
document.addEventListener("input", e => { if (e.target.id === "filter") { filter = e.target.value; render(); } });
$("#js").onclick = () => api("devJs");
loop();
refresh();
