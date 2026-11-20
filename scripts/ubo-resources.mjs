// Builds assets/ubo-resources.json: uBlock Origin's scriptlets and redirect resources in adblock-rust's format,
// what Brave assembles for its own resources component (brave/adblock-resources only publishes Brave's extras).
// Usage: git clone --depth 1 https://github.com/gorhill/uBlock <dir> && node scripts/ubo-resources.mjs <dir>
import { readFileSync, writeFileSync } from "node:fs";
import { join, extname } from "node:path";
import { pathToFileURL } from "node:url";

const ubo = process.argv[2];
if (!ubo) throw new Error("usage: node scripts/ubo-resources.mjs <uBlock checkout>");
const src = join(ubo, "src");
const { builtinScriptlets } = await import(pathToFileURL(join(src, "js/resources/scriptlets.js")));
const redirects = (await import(pathToFileURL(join(src, "js/redirect-resources.js")))).default;

const TRUSTED = 1; // adblock-rust permission bit; Leirk grants it to its built-in lists, as uBO and Brave do
const MIME = { ".js": "application/javascript", ".html": "text/html", ".txt": "text/plain", ".css": "text/css", ".json": "application/json",
  ".gif": "image/gif", ".png": "image/png", ".mp3": "audio/mp3", ".mp4": "video/mp4", ".xml": "application/xml" };
const list = a => (a === undefined ? [] : Array.isArray(a) ? a : [a]);
const out = [];

for (const s of builtinScriptlets) {
  out.push({
    name: s.name,
    aliases: list(s.aliases),
    kind: { mime: "application/javascript" },
    content: Buffer.from(s.fn.toString()).toString("base64"),
    dependencies: list(s.dependencies),
    permission: s.requiresTrust ? TRUSTED : 0,
  });
}
for (const [name, d] of redirects) {
  const mime = MIME[extname(name)] ?? (name === "empty" ? "text/plain" : undefined);
  if (!mime) throw new Error(`no mime for ${name}`);
  out.push({ name, aliases: list(d.alias), kind: { mime }, content: readFileSync(join(src, "web_accessible_resources", name)).toString("base64") });
}

const target = new URL("../assets/ubo-resources.json", import.meta.url);
writeFileSync(target, JSON.stringify(out));
console.log(`${builtinScriptlets.length} scriptlets, ${redirects.size} redirect resources -> ${target.pathname}`);
