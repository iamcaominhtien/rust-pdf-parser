// Browser checks that the page does not move while its pictures load
// (medium and friendly output; pictures are lazy-loaded).
//
// For each file, at a desktop and a phone width:
//  - every picture of the text has width and height attributes, so the
//    browser keeps its place before it loads;
//  - the position of every block is the same before and after all
//    pictures have loaded (scrolling through the page loads them);
//  - no loaded picture is distorted: its shown aspect ratio is its own;
// and opening a link to a heading near the end (in a new tab) lands on
// that heading at once, just below the top bar (no glide over the whole
// page), and stays there while the pictures around it load.
//
// usage: node tests/browser/check-stable.js file.medium.html file.friendly.html [...]
// Uses the Playwright package if installed, else playwright-core with the
// system Chrome (set P2H_CHROME_CHANNEL=chrome, as CI does).
let pw;
try {
  pw = require('playwright');
} catch (e) {
  pw = require('playwright-core');
}

const path = require('path');
const channel = process.env.P2H_CHROME_CHANNEL;
let problems = 0;

function fail(file, msg) {
  problems++;
  console.error(`FAIL ${file}: ${msg}`);
  process.exitCode = 1;
}

const BLOCKS = 'h1,h2,h3,h4,p,figure,table,.eq,aside,li,pre';

// Document positions of the blocks, rounded to a tenth of a pixel. Blocks
// in sticky or fixed boxes (contents column, top bar) follow the scroll
// and are left out.
function positions(blocks) {
  const pinned = (el) => {
    for (let a = el; a && a !== document.body; a = a.parentElement) {
      const p = getComputedStyle(a).position;
      if (p === 'sticky' || p === 'fixed') return true;
    }
    return false;
  };
  return [...document.querySelectorAll(blocks)]
    .filter((el) => !pinned(el))
    .map((el) => {
      const r = el.getBoundingClientRect();
      return Math.round((r.top + scrollY) * 10) / 10;
    });
}

async function loadAll(page) {
  await page.evaluate(async () => {
    for (let y = 0; y < document.documentElement.scrollHeight; y += 500) {
      scrollTo(0, y);
      await new Promise((r) => setTimeout(r, 20));
    }
  });
  await page
    .waitForFunction(() => [...document.images].every((img) => img.complete), null, { timeout: 15000 })
    .catch(() => {});
  await page.waitForTimeout(300);
}

async function stable(browser, file, width) {
  const context = await browser.newContext({ viewport: { width, height: 900 } });
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  await page.goto('file://' + path.resolve(file), { waitUntil: 'load' });
  await page.addStyleTag({ content: 'html{scroll-behavior:auto!important}' });
  await page.waitForTimeout(300);
  const before = await page.evaluate(positions, BLOCKS);
  const unsized = await page.evaluate(
    () =>
      [...document.images].filter(
        (img) => !img.closest('#lightbox') && !(img.getAttribute('width') && img.getAttribute('height'))
      ).length
  );
  await loadAll(page);
  const after = await page.evaluate(positions, BLOCKS);
  const unloaded = await page.evaluate(() => [...document.images].filter((img) => img.src && !img.complete).length);
  const distorted = await page.evaluate(() =>
    [...document.images]
      .filter((img) => img.naturalWidth && img.offsetWidth > 20 && img.offsetHeight > 20)
      .filter((img) => {
        const shown = img.offsetHeight / img.offsetWidth;
        const own = img.naturalHeight / img.naturalWidth;
        return getComputedStyle(img).objectFit !== 'contain' && Math.abs(shown - own) / own > 0.03;
      })
      .map((img) => `${img.alt.slice(0, 30)} (${img.offsetWidth}x${img.offsetHeight}, own ${img.naturalWidth}x${img.naturalHeight})`)
  );
  let moved = 0;
  let worst = 0;
  for (let i = 0; i < Math.min(before.length, after.length); i++) {
    const d = Math.abs(after[i] - before[i]);
    if (d > 1) moved++;
    worst = Math.max(worst, d);
  }
  await context.close();
  return { blocks: before.length, moved, worst, unsized, unloaded, distorted, errors, count: before.length === after.length };
}

// Open a link to a late heading in a new tab: it must land just below the
// top bar at once, and stay there once the pictures around it have loaded.
async function deepLink(browser, file) {
  const url = 'file://' + path.resolve(file);
  let context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  let page = await context.newPage();
  await page.goto(url, { waitUntil: 'load' });
  const id = await page.evaluate(() => {
    const heads = [...document.querySelectorAll('h2[id],h3[id]')];
    return heads.length ? heads[Math.floor(heads.length * 0.8)].id : null;
  });
  await context.close();
  if (!id) return null;
  context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  page = await context.newPage();
  await page.goto(url + '#' + id, { waitUntil: 'load' });
  const top = () => page.evaluate((id) => document.getElementById(id).getBoundingClientRect().top, id);
  await page.waitForTimeout(300);
  const first = await top();
  await page
    .waitForFunction(
      () =>
        [...document.images]
          .filter((img) => {
            const r = img.getBoundingClientRect();
            return r.bottom > -1500 && r.top < innerHeight + 1500;
          })
          .every((img) => img.complete),
      null,
      { timeout: 10000 }
    )
    .catch(() => {});
  await page.waitForTimeout(500);
  const later = await top();
  await context.close();
  return { id, first: Math.round(first), later: Math.round(later) };
}

(async () => {
  const files = process.argv.slice(2);
  if (!files.length) {
    console.error('usage: node check-stable.js file.html [...]');
    process.exit(2);
  }
  const browser = await pw.chromium.launch(channel ? { channel } : {});
  for (const file of files) {
    const desk = await stable(browser, file, 1440);
    const phone = await stable(browser, file, 390);
    const link = await deepLink(browser, file);
    console.log(
      `${file}: ${desk.blocks} blocks; moved while pictures load: ${desk.moved} (desktop), ${phone.moved} (phone); ` +
        `link #${link ? link.id : '-'} at ${link ? link.first : '-'}px -> ${link ? link.later : '-'}px`
    );
    for (const [name, r] of [
      ['desktop', desk],
      ['phone', phone],
    ]) {
      if (r.errors.length) fail(file, `${name}: page errors: ${r.errors.join('; ')}`);
      if (r.unsized) fail(file, `${name}: ${r.unsized} pictures without width and height`);
      if (r.unloaded) fail(file, `${name}: ${r.unloaded} pictures did not load`);
      if (!r.count) fail(file, `${name}: the number of blocks changed`);
      if (r.moved) fail(file, `${name}: ${r.moved} blocks moved while pictures loaded (up to ${r.worst.toFixed(1)}px)`);
      if (r.distorted.length) fail(file, `${name}: distorted pictures: ${r.distorted.slice(0, 3).join('; ')}`);
    }
    if (link && (Math.abs(link.later - link.first) > 1 || link.first < 0 || link.first > 100))
      fail(file, `link to #${link.id} landed at ${link.first}px and moved to ${link.later}px`);
  }
  await browser.close();
  if (problems) console.error(`${problems} problem(s)`);
})();
