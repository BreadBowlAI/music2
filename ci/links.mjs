#!/usr/bin/env node
// ci/links.mjs: the link check of music2's docs. ci/guide.sh check runs it, and so do ci/pr.sh's
// docs and lint steps.
//
//   node ci/links.mjs
//
// In short: it reads every Markdown file git knows (tracked, or new and not ignored), and GitHub's
// issue forms (.github/ISSUE_TEMPLATE/*.yml, for their full URLs alone), and checks
//   - every relative link names a file or folder that is there, and every #anchor in it a heading
//     of that Markdown file (GitHub's ids, which mdBook makes the same for plain headings);
//   - every https://github.com/BreadBowlAI/music2/blob/main/<path> (or /tree/main/) names a path
//     that is there, and its #anchor a heading too;
//   - every https://breadbowlai.github.io/music2/<page>.html names a page of the guide;
//   - the guide (docs/guide/): every page is in SUMMARY.md once, SUMMARY.md names only pages that
//     are there, each page starts with "# Title" (after one comment line, on a page made from code)
//     and then **In short**, a page links out of the
//     guide only by full URL, and each {{#include}} names a file that is there;
//   - templates/song-agents.md (every new song's AGENTS.md) has no relative link and is 80 lines
//     at most.
// Code blocks and code spans are skipped. It never fetches a web page: the answer is the same on
// any machine, any day. Prints one line per problem, file:line first, then a total. Exit 0 when
// there is none, 1 when there are some, 2 when it couldn't start.

import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const REPO = "https://github.com/BreadBowlAI/music2/";
const SITE = "https://breadbowlai.github.io/music2/";
const GUIDE = "docs/guide/";
const AGENTS = "templates/song-agents.md";
const AGENTS_MAX = 80;

let files;
try {
  files = execFileSync("git", ["-c", "core.quotePath=false", "ls-files", "-z", "--cached", "--others", "--exclude-standard"], {
    cwd: ROOT,
    encoding: "utf8",
    maxBuffer: 64 << 20,
  })
    .split("\0")
    .filter((f) => f && fs.existsSync(path.join(ROOT, f)));
} catch (e) {
  console.error(`ci/links.mjs: git ls-files failed: ${e.message}`);
  process.exit(2);
}
const known = new Set(files);
const folders = new Set();
for (const f of files) {
  for (let d = path.posix.dirname(f); d !== "."; d = path.posix.dirname(d)) folders.add(d);
}
const markdown = files.filter((f) => f.endsWith(".md")).sort();
const forms = files.filter((f) => f.startsWith(".github/ISSUE_TEMPLATE/") && f.endsWith(".yml")).sort();

const problems = [];
const say = (file, line, words) => problems.push(`${file}:${line}: ${words}`);

// The text of a Markdown file with its fenced code blocks blanked (lines kept, so numbers hold).
const texts = new Map();
function text(file) {
  if (!texts.has(file)) {
    const lines = fs.readFileSync(path.join(ROOT, file), "utf8").split("\n");
    let fence = null;
    const out = lines.map((l) => {
      const m = /^ {0,3}(`{3,}|~{3,})/.exec(l);
      if (fence) {
        if (m && m[1][0] === fence[0] && m[1].length >= fence.length && /^ {0,3}[`~]+\s*$/.test(l)) fence = null;
        return "";
      }
      if (m) {
        fence = m[1];
        return "";
      }
      return l;
    });
    texts.set(file, out);
  }
  return texts.get(file);
}

// A line without its code spans (`code`, ``code with ` inside``).
function withoutCode(line) {
  return line.replace(/(`+)(?:(?!\1)[\s\S])*?\1/g, (m) => " ".repeat(m.length));
}

// GitHub's id for a heading: its text as shown, in lower case, without punctuation but - and _,
// spaces as -. A second heading with the same id gets -1, a third -2.
function slug(heading) {
  const shown = heading
    .replace(/!\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/<[^>]+>/g, "")
    .replace(/`/g, "")
    .replace(/(\*\*|__|\*|_(?=\S)|(?<=\S)_)/g, (m) => (m === "_" ? "_" : ""))
    .trim()
    .toLowerCase();
  return shown.replace(/[^\p{L}\p{M}\p{N}\p{Pc} -]/gu, "").replace(/ /g, "-");
}

const anchorsOf = new Map();
function anchors(file) {
  if (!anchorsOf.has(file)) {
    const seen = new Map();
    const ids = new Set();
    for (const l of text(file)) {
      const h = /^ {0,3}#{1,6}\s+(.*?)\s*#*\s*$/.exec(l);
      if (h) {
        const base = slug(h[1]);
        const n = seen.get(base) ?? 0;
        seen.set(base, n + 1);
        ids.add(n === 0 ? base : `${base}-${n}`);
      }
      for (const m of l.matchAll(/<a\s+(?:name|id)="([^"]+)"/g)) ids.add(m[1]);
    }
    anchorsOf.set(file, ids);
  }
  return anchorsOf.get(file);
}

// Every link of a line: inline links and pictures, reference definitions, autolinks, and HTML's
// href and src.
function linksOf(line) {
  const out = [];
  const l = withoutCode(line);
  for (const m of l.matchAll(/\]\(\s*<?([^)\s>]+)>?(?:\s+"[^"]*")?\s*\)/g)) out.push(m[1]);
  const def = /^ {0,3}\[[^\]]+\]:\s+<?(\S+?)>?(\s|$)/.exec(l);
  if (def) out.push(def[1]);
  for (const m of l.matchAll(/<(https?:\/\/[^>\s]+)>/g)) out.push(m[1]);
  for (const m of l.matchAll(/\b(?:href|src)="([^"]+)"/g)) out.push(m[1]);
  return out;
}

// check <file> <line> <path in the repo> <anchor or null> <what the link said>: a link to a
// path in the tree, and its anchor when it names a Markdown file.
function checkTarget(file, n, target, anchor, link) {
  target = target.replace(/\/+$/, "");
  if (target === "" || target === ".") {
    if (anchor) say(file, n, `${link}: an anchor on the repo's top folder`);
    return;
  }
  if (known.has(target)) {
    if (anchor && target.endsWith(".md") && !/^L\d+(-L\d+)?$/.test(anchor) && !anchors(target).has(anchor)) {
      say(file, n, `${link}: ${target} has no heading #${anchor}`);
    }
    return;
  }
  if (folders.has(target)) {
    if (anchor) say(file, n, `${link}: #${anchor} on a folder`);
    return;
  }
  say(file, n, `${link}: there's no ${target}`);
}

// The guide's pages, by their path under docs/guide/, as SUMMARY.md lists them.
const guidePages = markdown.filter((f) => f.startsWith(GUIDE) && f !== `${GUIDE}SUMMARY.md`);
const summary = [];
if (known.has(`${GUIDE}SUMMARY.md`)) {
  text(`${GUIDE}SUMMARY.md`).forEach((l, i) => {
    for (const m of withoutCode(l).matchAll(/\]\(([^)\s]+)\)/g)) summary.push({ page: m[1], line: i + 1 });
  });
}
const sitePages = new Set(summary.map((s) => s.page.replace(/\.md$/, ".html")));

for (const file of [...markdown, ...forms]) {
  const inGuide = file.startsWith(GUIDE);
  const isForm = !file.endsWith(".md");
  text(file).forEach((line, i) => {
    const n = i + 1;
    for (const link of linksOf(line)) {
      if (link.startsWith(SITE)) {
        const [page, anchor] = link.slice(SITE.length).split("#");
        if (page !== "" && page !== "index.html" && !sitePages.has(page)) {
          say(file, n, `${link}: the guide has no page ${page} (docs/guide/SUMMARY.md)`);
        } else if (anchor && page !== "" && page !== "index.html") {
          const md = `${GUIDE}${page.replace(/\.html$/, ".md")}`;
          if (known.has(md) && !anchors(md).has(anchor)) say(file, n, `${link}: ${md} has no heading #${anchor}`);
        }
        continue;
      }
      const repo = /^https:\/\/github\.com\/BreadBowlAI\/music2\/(?:blob|tree)\/main\/([^?#]*)(?:#(.*))?$/.exec(link);
      if (repo) {
        checkTarget(file, n, decodeURIComponent(repo[1]), repo[2] ?? null, link);
        continue;
      }
      if (/^[a-z][a-z0-9+.-]*:/i.test(link)) continue; // https:, mailto: and the like: not fetched
      if (link.startsWith("//") || isForm) continue;
      const [p, anchor] = link.split("#");
      if (p === "") {
        if (anchor && !anchors(file).has(anchor)) say(file, n, `${link}: no heading #${anchor} here`);
        continue;
      }
      if (file === AGENTS) say(file, n, `${link}: a relative link, which a song's folder can't follow (use a full URL)`);
      let target;
      try {
        target = p.startsWith("/") ? p.slice(1) : path.posix.join(path.posix.dirname(file), decodeURIComponent(p));
      } catch {
        say(file, n, `${link}: not a path`);
        continue;
      }
      target = path.posix.normalize(target);
      if (target.startsWith("../")) {
        say(file, n, `${link}: outside the repo`);
        continue;
      }
      if (inGuide && !target.startsWith(GUIDE)) {
        say(file, n, `${link}: a guide page links out of docs/guide/ only by full URL (${REPO}blob/main/${target}), as the site holds the guide alone`);
        continue;
      }
      checkTarget(file, n, target, anchor ?? null, link);
    }
    // A guide page's includes: a whole line, naming a file that is there.
    if (inGuide) {
      for (const m of line.matchAll(/\{\{#(\w+)\s*([^}]*)\}\}/g)) {
        if (m[1] !== "include" || !/^\s*\{\{#include [^}\s]+\}\}\s*$/.test(line)) {
          say(file, n, `${m[0]}: the guide takes only {{#include <path>}}, alone on its line (m2 docs fills in nothing else)`);
          continue;
        }
        const target = path.posix.normalize(path.posix.join(path.posix.dirname(file), m[2].trim()));
        if (!known.has(target)) say(file, n, `${m[0]}: there's no ${target}`);
      }
    }
  });
}

// The guide: each page once in SUMMARY.md, each starting with its title and In short.
if (guidePages.length > 0 || summary.length > 0) {
  if (!known.has(`${GUIDE}SUMMARY.md`)) say(`${GUIDE}SUMMARY.md`, 1, "the guide has no SUMMARY.md");
  const listed = new Map();
  for (const s of summary) {
    const target = path.posix.normalize(path.posix.join(GUIDE, s.page));
    if (!known.has(target)) say(`${GUIDE}SUMMARY.md`, s.line, `${s.page}: there's no ${target}`);
    if (listed.has(target)) say(`${GUIDE}SUMMARY.md`, s.line, `${s.page} is listed twice (first on line ${listed.get(target)})`);
    else listed.set(target, s.line);
    if (path.posix.dirname(target) !== GUIDE.slice(0, -1)) {
      say(`${GUIDE}SUMMARY.md`, s.line, `${s.page}: the guide is flat, every page in docs/guide/ (m2 docs names a page by its file name)`);
    }
  }
  for (const page of guidePages) {
    if (!listed.has(page)) say(page, 1, "not in docs/guide/SUMMARY.md: every page of the guide is listed there once");
    const lines = expanded(page);
    // A page made from code says so on its first line, in a comment the site doesn't show.
    const title = lines.findIndex((l) => l.trim() !== "" && !/^<!--.*-->\s*$/.test(l));
    if (title < 0 || !/^# \S/.test(lines[title])) {
      say(page, 1, 'a guide page starts with its title, "# Title"');
      continue;
    }
    const next = lines.slice(title + 1).find((l) => l.trim() !== "");
    if (!next || !next.startsWith("**In short**")) say(page, title + 2, "after its title, a guide page says **In short** (AGENTS.md, rule 10)");
  }
}

// The lines of a guide page with each whole-line {{#include}} filled in, as mdBook and m2 docs
// show it.
function expanded(page) {
  return fs
    .readFileSync(path.join(ROOT, page), "utf8")
    .split("\n")
    .flatMap((l) => {
      const m = /^\s*\{\{#include ([^}\s]+)\}\}\s*$/.exec(l);
      if (!m) return [l];
      const target = path.join(ROOT, path.dirname(page), m[1]);
      return fs.existsSync(target) ? fs.readFileSync(target, "utf8").split("\n") : [l];
    });
}

// Every new song's AGENTS.md: short, and read where no relative link leads.
if (known.has(AGENTS)) {
  const lines = fs.readFileSync(path.join(ROOT, AGENTS), "utf8").replace(/\n$/, "").split("\n");
  if (lines.length > AGENTS_MAX) say(AGENTS, AGENTS_MAX + 1, `${lines.length} lines: every new song gets this page, so it is ${AGENTS_MAX} lines at most`);
}

for (const p of problems) console.log(p);
console.log(`${markdown.length} Markdown files and ${forms.length} issue forms, ${problems.length === 0 ? "every link good" : `${problems.length} problem${problems.length === 1 ? "" : "s"}`}`);
process.exit(problems.length === 0 ? 0 : 1);
