// CSP guard for the built page: every inline <script> in index.html must have its sha256 listed in the
// Content-Security-Policy script-src directive, or the browser blocks it. Written independently of
// version-dist.mjs's hashing so a bug there cannot pass its own check. version-dist's CLI runs it after
// every Trunk build (post_build), so a mismatch fails the build.
// Usage: node scripts/web/csp-check.mjs [dir]   (default: dist)
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

/** Inline scripts whose hash the page's CSP does not list, as `{ hash, head }` (empty when all are covered). */
export function cspMissing(html) {
  const policy = /<meta\b[^>]*http-equiv="Content-Security-Policy"[^>]*>/i.exec(html)?.[0];
  if (!policy) throw new Error("csp-check: the page has no Content-Security-Policy meta");
  const directive = /script-src\s+([^;"]+)/.exec(policy)?.[1];
  if (!directive) throw new Error("csp-check: the CSP has no script-src directive");
  if (/'unsafe-inline'/.test(directive)) throw new Error("csp-check: script-src allows 'unsafe-inline'");
  const listed = new Set(directive.match(/'sha256-[A-Za-z0-9+/=]+'/g) ?? []);
  const missing = [];
  for (const [, attrs, body] of html.matchAll(/<script\b([^>]*)>([\s\S]*?)<\/script>/gi)) {
    if (/\bsrc\s*=/i.test(attrs)) continue;
    const hash = `'sha256-${createHash("sha256").update(body, "utf8").digest("base64")}'`;
    if (!listed.has(hash)) missing.push({ hash, head: body.trim().slice(0, 60) });
  }
  return missing;
}

/** Throws with every uncovered inline script named, so a build or a test stops on it. */
export function assertCsp(html, where = "index.html") {
  const missing = cspMissing(html);
  if (missing.length)
    throw new Error(
      `csp-check: ${missing.length} inline script(s) in ${where} not in script-src: ` +
        missing.map((m) => `${m.hash} (${JSON.stringify(m.head)})`).join(", "),
    );
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const dir = process.argv[2] ?? "dist";
  assertCsp(readFileSync(join(dir, "index.html"), "utf8"), join(dir, "index.html"));
  console.log(`csp-check: every inline script in ${dir}/index.html is hashed in script-src`);
}
