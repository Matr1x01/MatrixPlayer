#!/usr/bin/env node
// Unpacks the bundled design file ("Fermata — Linux Music Player.html").
//
// The design is a two-level bundle: the outer document carries one gzipped,
// base64 page per board, and each page is itself a bundle carrying fonts and
// scripts. This script writes:
//   design/extracted/NN-<board>.html  readable page templates (gitignored)
//   src/assets/fonts/*.woff2          the variable fonts the design uses
//
// Usage: node scripts/extract-design.mjs [path-to-design.html]

import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';

const root = path.resolve(path.dirname(new URL(import.meta.url).pathname), '..');
const input = process.argv[2] ?? path.join(root, 'Fermata — Linux Music Player.html');
const boardsDir = path.join(root, 'design', 'extracted');
const fontsDir = path.join(root, 'src', 'assets', 'fonts');

function island(src, type) {
  const m = src.match(new RegExp(`<script type="__bundler/${type}">\\s*([\\s\\S]*?)\\s*</script>`));
  if (!m) throw new Error(`bundle island "${type}" not found`);
  return JSON.parse(m[1]);
}

function decode(entry) {
  const bytes = Buffer.from(entry.data, 'base64');
  return entry.compressed ? zlib.gunzipSync(bytes) : bytes;
}

const outer = fs.readFileSync(input, 'utf8');
const manifest = island(outer, 'manifest');
const order = island(outer, 'page_order');
const template = island(outer, 'template');
const titles = Object.fromEntries(
  [...template.matchAll(/about:blank#([0-9a-f-]{36})" title="([^"]+)"/g)].map((m) => [m[1], m[2]]),
);

fs.mkdirSync(boardsDir, { recursive: true });
fs.mkdirSync(fontsDir, { recursive: true });

let fontsWritten = false;
order.forEach((uuid, i) => {
  const page = decode(manifest[uuid]).toString('utf8');
  const pageManifest = island(page, 'manifest');
  let html = island(page, 'template');

  // Font subsets are identified by the unicode-range of their @font-face rule.
  const faces = [...html.matchAll(/@font-face\s*\{([^}]*)\}/g)].map((m) => m[1]);
  for (const face of faces) {
    const family = face.match(/font-family:\s*'([^']+)'/)?.[1];
    const uuidRef = face.match(/url\("([0-9a-f-]{36})"\)/)?.[1];
    const range = face.match(/unicode-range:\s*([^;]+);/)?.[1] ?? '';
    if (!family || !uuidRef || fontsWritten) continue;
    const subset = subsetName(range);
    const file = `${family.toLowerCase().replace(/\s+/g, '-')}-${subset}.woff2`;
    fs.writeFileSync(path.join(fontsDir, file), decode(pageManifest[uuidRef]));
  }
  fontsWritten = true;

  html = html.replace(/@font-face\s*\{[^}]*\}\s*/g, '');
  for (const id of Object.keys(pageManifest)) html = html.split(id).join(`asset:${id.slice(0, 8)}`);
  const name = `${String(i).padStart(2, '0')}-${titles[uuid].replace(/[^a-zA-Z0-9]+/g, '-').replace(/-+$/, '')}.html`;
  fs.writeFileSync(path.join(boardsDir, name), html);
  console.log(`board  ${name}`);
});

for (const f of fs.readdirSync(fontsDir)) console.log(`font   src/assets/fonts/${f}`);

function subsetName(range) {
  if (range.startsWith('U+0000-00FF')) return 'latin';
  if (range.startsWith('U+0100-02BA')) return 'latin-ext';
  if (range.startsWith('U+0102-0103')) return 'vietnamese';
  if (range.startsWith('U+0460-052F')) return 'cyrillic-ext';
  if (range.startsWith('U+0301, U+0400')) return 'cyrillic';
  if (range.startsWith('U+0370-0377')) return 'greek';
  throw new Error(`unknown unicode-range: ${range.slice(0, 40)}`);
}
