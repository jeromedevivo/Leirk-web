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

const MODULES = [["tech", "Technologies"], ["docker", "Docker"], ["ports", "Serveurs web locaux"], ["tools", "Outils"]];
const ASK = { tech: "devTech", docker: "devDocker", ports: "devPorts" };
const EVERY = { docker: 5000, ports: 8000 };
let state = { dev: false, modules: {} }, data = {}, tab = "tech", logsFor = "", lastAsk = 0;
const openStacks = new Set();

document.addEventListener("toggle", e => { const p = e.target.dataset?.stack; if (p != null) e.target.open ? openStacks.add(p) : openStacks.delete(p); }, true);

async function api(c, v) {
  try {
    const r = await fetch(`http://127.0.0.1:${LEIRK.port}/`, { method: "POST", headers: { "content-type": "application/json", "x-leirk-token": LEIRK.token }, body: JSON.stringify({ c, v, tab: nonce }), keepalive: true });
    const j = await r.json();
    for (const [kind, d] of j.outbox || []) data[kind] = d;
    state = j;
    return j;
  } catch { state = { offline: true }; }
}
const mods = () => MODULES.filter(([m]) => state.modules?.[m] !== false);
function refresh() { if (ASK[tab]) api(ASK[tab]); lastAsk = Date.now(); }

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
  if (body.contains(document.activeElement) && /INPUT|TEXTAREA/.test(document.activeElement.tagName)) return;
  const html = paint(), scroll = body.scrollTop;
  if (html === body.dataset.last) return;
  body.dataset.last = html; body.innerHTML = html; body.scrollTop = scroll;
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
    case "docker": {
      const k = d.docker;
      if (!k) return `<p class="muted">Lecture de Docker…</p>`;
      if (k.error) return `<p class="muted">Docker ne répond pas : ${esc(k.error)}</p>`;
      const act = (ids, op, label, prim) => `<button class="btn${prim ? " prim" : ""}" data-act="docker" data-id="${esc(ids)}" data-op="${op}">${label}</button>`;
      const card = c => `<div class="card">
        <div class="bar"><span class="dot ${c.state === "running" ? "ok" : ""}"></span><b>${esc(c.service || c.name)}</b><span class="muted">${esc(c.image)}</span></div>
        <div class="muted">${esc(c.status)}${c.cpu ? ` · CPU ${esc(c.cpu)} · RAM ${esc(c.mem)}` : ""}</div>
        <div class="chips">${c.ports.map(p => `<button class="btn" data-act="port" data-port="${esc(p)}">localhost:${esc(p)}</button>`).join("")}<span class="grow"></span>
        ${c.state === "running" ? act(c.id, "restart", "Redémarrer") + act(c.id, "stop", "Arrêter") : act(c.id, "start", "Démarrer", true)}
        <button class="btn" data-act="logs" data-id="${esc(c.id)}">Logs</button></div>${logsFor === c.id && d.logs?.id === c.id ? `<pre>${esc(d.logs.text)}</pre>` : ""}</div>`;
      const groups = {};
      k.containers.forEach(c => (groups[c.project] ||= []).push(c));
      const stacks = Object.entries(groups).filter(([p]) => p).map(([p, l]) => {
        const on = l.filter(c => c.state === "running").length, ids = l.map(c => c.id).join(" ");
        return `<details class="stack" data-stack="${esc(p)}" ${openStacks.has(p) ? "open" : ""}><summary><span class="dot ${on ? "ok" : ""}"></span><b>${esc(p)}</b><span class="muted">${on}/${l.length} conteneurs actifs</span><span class="grow"></span>
          ${on ? act(ids, "restart", "Tout redémarrer") + act(ids, "stop", "Tout arrêter") : ""}${on < l.length ? act(ids, "start", "Tout démarrer", true) : ""}</summary>${l.map(card).join("")}</details>`;
      });
      const solo = groups[""] || [];
      return `<div class="bar"><b>${stacks.length} projet${stacks.length > 1 ? "s" : ""} · ${k.containers.length} conteneurs</b></div>${stacks.join("")}`
        + (solo.length ? `<h4>Conteneurs indépendants</h4>${solo.map(card).join("")}` : "");
    }
    case "ports": {
      const p = d.ports || [];
      return `<div class="bar"><b>${p.length} serveur${p.length > 1 ? "s" : ""} web en écoute</b><span class="grow"></span><button class="btn" data-act="ports">Actualiser</button></div>`
        + (p.length ? "" : `<p class="muted">Aucun serveur web local. Lancez par exemple « npm run dev » : il apparaîtra ici.</p>`) + p.map(x => `<div class="row"><b style="min-width:52px">${esc(x.port)}</b><span class="t">${esc(x.process || "?")}</span><span class="k">pid ${esc(x.pid)}</span><button class="btn" data-act="port" data-port="${esc(x.port)}">Ouvrir</button></div>`).join("");
    }
    case "tools": {
      const envs = (state.envs || "").split("\n").map(s => s.trim()).filter(Boolean);
      return `<h4>Environnements</h4>${envs.length ? `<div class="chips">${envs.map(h => `<button class="btn" data-act="env" data-host="${esc(h)}">${esc(h)}</button>`).join("")}</div>` : `<p class="muted">Ajoutez vos hôtes dans Leirk › Réglages › Mode développeur.</p>`}
        <h4>Convertisseurs</h4><textarea id="tin" rows="5" placeholder="Collez un JWT, du Base64, du JSON, un timestamp, une URL…"></textarea>
        <div class="chips">${[["jwt", "Décoder un JWT", "Affiche le contenu d'un jeton de connexion (eyJ…) et sa date d'expiration."], ["b64", "Texte → Base64"], ["d64", "Base64 → texte"], ["url", "Encoder pour une URL", "« é & ? » devient « %C3%A9%20%26%20%3F », utilisable dans un lien."], ["durl", "Décoder une URL", "« %C3%A9%20 » redevient « é »."], ["json", "Formater du JSON"], ["ts", "Timestamp ↔ date", "1700000000 ↔ date lisible."], ["uuid", "Générer un identifiant unique", "Un UUID aléatoire, utile comme identifiant de test."]].map(([k, l, t]) => `<button class="btn" data-act="tool" data-k="${k}" ${t ? `title="${esc(t)}"` : ""}>${l}</button>`).join("")}</div><pre id="tout"> </pre>`;
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
  e.preventDefault(); // a button inside a project header must not fold it
  const ds = a.dataset;
  const later = c => setTimeout(() => api(c), 150);
  switch (ds.act) {
    case "tech": data.tech = null; return api("devTech");
    case "docker": return api("devDockerAct", { id: ds.id, act: ds.op });
    case "logs": logsFor = logsFor === ds.id ? "" : ds.id; if (logsFor) api("devDockerLogs", ds.id); return render();
    case "port": return api("open", `http://localhost:${ds.port}`);
    case "ports": return api("devPorts");
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
$("#js").onclick = () => api("devJs");
loop();
refresh();
