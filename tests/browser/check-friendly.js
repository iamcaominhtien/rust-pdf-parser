// Browser checks of friendly output (the parts only a browser can test).
//
// For each file:
//  - the page scripts run without errors, in scroll and in book view;
//  - no horizontal page scroll at 320px and 390px (only equations, tables
//    and code may scroll in their own box);
//  - themes: the system's dark mode is followed when nothing is stored, a
//    chosen theme survives a reload and wins over the system;
//  - the contents column can be hidden (remembered) and opens as a drawer
//    on phones;
//  - footnotes in the margin are never above the line of their mark;
//  - book view: pages are laid out, the buttons and keys turn them, a page
//    turn takes its time and ends, the page count covers the text once all
//    images have loaded, the recorded page sound plays (and not when it is
//    switched off), the article returns to the scroll view,
//    and a translated (longer) text is paginated again;
//  - wide equations, tables and pictures of tables are never cut off: on a
//    phone and in the book view each is inside its column or in a box that
//    scrolls sideways, fades at the cut edge and reaches the end; an equation
//    number stays beside its picture; no picture of an equation or a table is
//    shrunk below 70%;
//  - book view layout: a figure or table is on one page with its caption
//    (except one too long for a page, which may split), no heading ends a
//    page, and no page is more than 60% empty (except before a heading, or
//    a float of more than half a page, and the last page).
//
// usage: node tests/browser/check-friendly.js file.friendly.html [...]
// Uses the Playwright package if installed, else playwright-core with the
// system Chrome (set P2H_CHROME_CHANNEL=chrome, as CI does).
let pw;
try {
  pw = require('playwright');
} catch (e) {
  pw = require('playwright-core');
}

const channel = process.env.P2H_CHROME_CHANNEL;
const problems = [];

function fail(file, msg) {
  problems.push(`${file}: ${msg}`);
  console.error(`FAIL ${file}: ${msg}`);
  process.exitCode = 1;
}

async function open(browser, file, options, storage) {
  const context = await browser.newContext(options);
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  if (storage) {
    await page.addInitScript((s) => {
      for (const [k, v] of Object.entries(s)) localStorage.setItem(k, v);
    }, storage);
  }
  await page.goto('file://' + require('path').resolve(file), { waitUntil: 'load' });
  await page.addStyleTag({ content: 'html{scroll-behavior:auto!important}' });
  return { context, page, errors };
}

async function overflow(browser, file, width) {
  const { context, page } = await open(browser, file, { viewport: { width, height: 800 } });
  const extra = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  await context.close();
  return extra;
}

async function themes(browser, file) {
  const dark = await open(browser, file, { viewport: { width: 1280, height: 800 }, colorScheme: 'dark' });
  const darkInk = await dark.page.evaluate(() => getComputedStyle(document.body).backgroundColor);
  await dark.page.click('[data-theme-pick="white"]');
  await dark.page.reload();
  const chosen = await dark.page.evaluate(() => [
    document.documentElement.getAttribute('data-theme'),
    getComputedStyle(document.body).backgroundColor,
    document.querySelector('[data-theme-pick="white"]').getAttribute('aria-pressed'),
  ]);
  await dark.context.close();
  return { darkInk, chosen, errors: dark.errors };
}

async function contents(browser, file) {
  const desk = await open(browser, file, { viewport: { width: 1440, height: 900 } });
  const before = await desk.page.evaluate(() => document.getElementById('article').offsetWidth);
  await desk.page.click('#toc-toggle');
  const after = await desk.page.evaluate(() => [
    document.getElementById('article').offsetWidth,
    document.getElementById('toc-toggle').getAttribute('aria-expanded'),
  ]);
  await desk.page.reload();
  const kept = await desk.page.evaluate(() => document.documentElement.getAttribute('data-toc'));
  await desk.context.close();
  const phone = await open(browser, file, { viewport: { width: 390, height: 844 } });
  await phone.page.click('#toc-toggle');
  const drawer = await phone.page.evaluate(() => document.getElementById('toc-drawer').open);
  await phone.page.keyboard.press('Escape');
  const closed = await phone.page.evaluate(() => !document.getElementById('toc-drawer').open);
  await phone.context.close();
  return { before, after, kept, drawer, closed, errors: desk.errors.concat(phone.errors) };
}

// On a wide screen footnotes sit in the margin: never above the line of
// their mark, even after the lazy images above it have loaded.
async function sidenotes(browser, file) {
  const { context, page, errors } = await open(browser, file, { viewport: { width: 1600, height: 900 } });
  await page.evaluate(async () => {
    for (let y = 0; y < document.body.scrollHeight; y += 600) {
      scrollTo(0, y);
      await new Promise((r) => setTimeout(r, 30));
    }
  });
  await page.waitForTimeout(800);
  const above = await page.evaluate(() =>
    [...document.querySelectorAll('aside.footnote.side')]
      .map((note) => {
        const mark = note.id && document.querySelector(`a[href="#${CSS.escape(note.id)}"]`);
        return mark ? [note.id, note.getBoundingClientRect().top - mark.getBoundingClientRect().top] : null;
      })
      .filter((item) => item && item[1] < -4)
      .map(([id, dy]) => `${id} ${Math.round(dy)}px`)
  );
  await context.close();
  return { above, errors };
}

// The page turn sound: the recording is embedded and decodes; a turn plays
// it with the sound on and stays silent with it off.
async function pageSound(browser, file) {
  const { context, page, errors } = await open(browser, file, { viewport: { width: 1440, height: 900 } });
  await page.evaluate(() => {
    window.__plays = 0;
    const start = AudioBufferSourceNode.prototype.start;
    AudioBufferSourceNode.prototype.start = function () {
      if (this.buffer && this.buffer.duration > 0.5) window.__plays++;
      return start.apply(this, arguments);
    };
  });
  await page.evaluate(() => document.querySelector('[data-view-pick="book"]').click());
  await page.waitForTimeout(600);
  await page.click('.bk-row .bk-nav.next');
  await page.waitForFunction(() => !window.p2hBook.state().busy, null, { timeout: 5000 });
  const on = await page.evaluate(() => window.__plays);
  await page.click('.bk-sound');
  await page.click('.bk-row .bk-nav.next');
  await page.waitForFunction(() => !window.p2hBook.state().busy, null, { timeout: 5000 });
  const off = (await page.evaluate(() => window.__plays)) - on;
  await context.close();
  return { on, off, errors };
}

// The book's pages, measured in the laid-out article: captions apart from
// their figure or table, headings at the foot of a page, and pages that are
// mostly empty. A page is a column of the article.
function layoutProblems() {
  const art = document.querySelector('.bk-window > article');
  const cs = getComputedStyle(art);
  const step = parseFloat(cs.columnWidth) + parseFloat(cs.columnGap);
  const th = art.clientHeight;
  const box = art.getBoundingClientRect();
  const col = (x) => Math.max(0, Math.floor((x - box.left + 2) / step));
  const pages = window.p2hBook.state().pages;
  const fill = new Array(pages).fill(0);
  const leaf = 'p,li,h1,h2,h3,h4,figcaption,img,tr,pre,.eq,.algo,.authors,.kicker,aside,.abstract-label,.ln';
  art.querySelectorAll(leaf).forEach((e) => {
    if (e.matches('li') && e.querySelector('p,li')) return;
    for (const r of e.getClientRects()) {
      const c = col(r.left);
      if (r.height && c < pages) fill[c] = Math.max(fill[c], (r.bottom - box.top) / th);
    }
  });
  // What starts each page: a heading, or a float of over half a page.
  const excused = new Array(pages).fill(false);
  for (const k of art.children) {
    const r = k.getClientRects()[0];
    if (!r || r.top - box.top > th * 0.12) continue;
    const c = col(r.left);
    const heading = /^H[1-4]$/.test(k.tagName);
    const big = k.matches('figure.float') && k.getBoundingClientRect().height > th * 0.5;
    if (c < pages && (heading || big)) excused[c] = true;
  }
  const empty = [];
  for (let c = 0; c < pages - 1; c++) if (fill[c] < 0.4 && !excused[c + 1]) empty.push(`${c + 1} (${Math.round(fill[c] * 100)}%)`);
  const apart = [];
  art.querySelectorAll('figure.float').forEach((f) => {
    const cap = f.querySelector('figcaption');
    const body = f.querySelector('.plate,.tbl-wrap,.algo,img');
    if (!cap || !body || f.classList.contains('bk-split') || f.classList.contains('bk-box')) return;
    const cols = new Set([...cap.getClientRects()].map((r) => col(r.left)));
    body.querySelectorAll('img,tr,.ln').forEach((e) => {
      for (const r of e.getClientRects()) if (r.height) cols.add(col(r.left));
    });
    if (cols.size > 1) apart.push(f.id || f.className);
  });
  const orphans = [];
  art.querySelectorAll('h2,h3,h4').forEach((h) => {
    const rs = h.getClientRects();
    const next = h.nextElementSibling;
    const nr = next && next.getClientRects()[0];
    if (rs.length && nr && !/^H[2-4]$/.test(next.tagName) && col(nr.left) > col(rs[rs.length - 1].left)) orphans.push(h.textContent.slice(0, 30));
  });
  return { empty, apart, orphans };
}

// Wide content is never cut silently: every equation picture, table and
// picture of a table is inside its column, or inside a box that scrolls
// sideways (and is itself inside the column). An equation keeps its number
// beside the picture, and is not shrunk below 70% of its size. Runs in the
// scroll view or the book view, whichever is showing.
function clipProblems() {
  const book = document.documentElement.getAttribute('data-view') === 'book';
  const art = book ? document.querySelector('.bk-window > article') : document.getElementById('article');
  const box = art.getBoundingClientRect();
  let step = 0;
  let tw = 0;
  if (book) {
    const cs = getComputedStyle(art);
    tw = parseFloat(cs.columnWidth);
    step = tw + parseFloat(cs.columnGap);
  }
  // The right edge of the column (or of the viewport) that holds x.
  const limit = (x) => (book ? box.left + Math.max(0, Math.floor((x - box.left + 2) / step)) * step + tw : Math.min(box.right, innerWidth));
  const scrolls = (e) => {
    for (let n = e.parentElement; n && n !== art; n = n.parentElement) {
      const o = getComputedStyle(n).overflowX;
      if (o === 'auto' || o === 'scroll') return n;
    }
    return null;
  };
  const name = (e) => (e.closest('[id]') ? e.closest('[id]').id : '') + ' ' + e.tagName.toLowerCase();
  const clipped = [];
  const uncued = [];
  const numbers = [];
  const small = [];
  art.querySelectorAll('.eq-body img, .tbl-wrap table, .tbl-wrap > img').forEach((e) => {
    const r = e.getBoundingClientRect();
    if (!r.width || !r.height) return;
    const holder = scrolls(e);
    const hr = holder ? holder.getBoundingClientRect() : r;
    if (hr.right > limit(hr.left + 1) + 1 || hr.left < box.left - 1) clipped.push(`${name(e)} ${Math.round(hr.right - limit(hr.left + 1))}px`);
    if (holder && holder.scrollWidth > holder.clientWidth + 1) {
      // The box shows that there is more (a faded edge) and can be scrolled to the end.
      if (getComputedStyle(holder).maskImage === 'none' && getComputedStyle(holder).webkitMaskImage === 'none') uncued.push(name(e));
      const before = holder.scrollLeft;
      holder.scrollLeft = holder.scrollWidth;
      if (holder.scrollLeft <= before) clipped.push(`${name(e)} does not scroll`);
      holder.scrollLeft = before;
    }
    if (e.tagName === 'IMG') {
      const own = parseFloat(e.style.width);
      const full = e.style.width.endsWith('em') ? own * parseFloat(getComputedStyle(e).fontSize) : 0;
      if (full && r.width < full * 0.69) small.push(`${name(e)} ${Math.round((r.width / full) * 100)}%`);
    }
    const eq = e.closest('.eq');
    if (eq) {
      const no = eq.querySelector('.eqno');
      const body = eq.querySelector('.eq-body').getBoundingClientRect();
      if (no) {
        const nr = no.getBoundingClientRect();
        const mid = (nr.top + nr.bottom) / 2;
        if (nr.left < body.right - 1 || mid < body.top - 1 || mid > body.bottom + 1 || nr.right > limit(nr.left) + 1) numbers.push(name(e));
      }
    }
  });
  return { clipped, uncued, numbers, small };
}

// Clipped content on a phone, in the scroll view and in the book view.
async function phoneClips(browser, file) {
  const result = {};
  for (const view of ['scroll', 'book']) {
    const { context, page } = await open(browser, file, { viewport: { width: 390, height: 844 } });
    await page.evaluate(async () => {
      const imgs = [...document.images];
      imgs.forEach((img) => (img.loading = 'eager'));
      await Promise.all(imgs.map((img) => (img.complete ? 0 : new Promise((r) => (img.onload = img.onerror = r)))));
    });
    if (view === 'book') {
      await page.evaluate(() => document.querySelector('[data-view-pick="book"]').click());
      await page.waitForTimeout(800);
      await page
        .waitForFunction(() => [...document.querySelectorAll('.bk-window img')].every((img) => img.complete), null, { timeout: 10000 })
        .catch(() => {});
      await page.waitForTimeout(500);
    }
    await page.waitForTimeout(300);
    result[view] = await page.evaluate(clipProblems);
    await context.close();
  }
  return result;
}

async function book(browser, file) {
  const { context, page, errors } = await open(browser, file, { viewport: { width: 1440, height: 900 } }, { 'p2h-sound': 'off' });
  await page.evaluate(() => document.querySelector('[data-view-pick="book"]').click());
  await page.waitForTimeout(300);
  // Every image loads (none waits for its page) and the page count then
  // covers the whole text: the last column is on the last page.
  await page
    .waitForFunction(() => [...document.querySelectorAll('.bk-window img')].every((img) => img.complete), null, { timeout: 10000 })
    .catch(() => {});
  await page.waitForTimeout(500);
  const unloaded = await page.evaluate(() => [...document.querySelectorAll('.bk-window img')].filter((img) => !img.complete).length);
  const fit = await page.evaluate(() => {
    const art = document.querySelector('.bk-window > article');
    const cs = getComputedStyle(art);
    const step = parseFloat(cs.columnWidth) + parseFloat(cs.columnGap);
    return [Math.round((art.scrollWidth + parseFloat(cs.columnGap)) / step), window.p2hBook.state().pages];
  });
  const state = await page.evaluate(() => window.p2hBook.state());
  const layout = await page.evaluate(layoutProblems);
  layout.clips = await page.evaluate(clipProblems);
  const first = await page.textContent('.bk-count');
  await page.evaluate(() => document.querySelector('.bk-row .bk-nav.next').click());
  await page.waitForTimeout(150);
  const midTurn = await page.evaluate(() => window.p2hBook.state().busy);
  await page.waitForFunction(() => !window.p2hBook.state().busy, null, { timeout: 5000 });
  const second = await page.textContent('.bk-count');
  await page.focus('.bk-view');
  await page.keyboard.press('ArrowRight');
  await page.waitForFunction(() => !window.p2hBook.state().busy, null, { timeout: 5000 });
  const third = await page.textContent('.bk-count');
  // A translator replaces the text with a longer one: the book is paginated again.
  await page.evaluate(() => {
    document.querySelectorAll('#article p').forEach((p) => {
      p.textContent = p.textContent + ' ' + p.textContent.slice(0, Math.ceil(p.textContent.length * 0.2));
    });
  });
  await page.waitForTimeout(700);
  const translated = await page.evaluate(() => window.p2hBook.state());
  await page.evaluate(() => document.querySelector('[data-view-pick="scroll"]').click());
  const back = await page.evaluate(() => [
    document.documentElement.getAttribute('data-view'),
    document.querySelector('.layout > article') !== null,
  ]);
  await context.close();
  return { state, fit, layout, unloaded, first, midTurn, second, third, translated, back, errors };
}

(async () => {
  const files = process.argv.slice(2);
  if (!files.length) {
    console.error('usage: node check-friendly.js file.html [...]');
    process.exit(2);
  }
  const browser = await pw.chromium.launch(channel ? { channel } : {});
  for (const file of files) {
    const narrow = await overflow(browser, file, 320);
    const phone = await overflow(browser, file, 390);
    const theme = await themes(browser, file);
    const toc = await contents(browser, file);
    const side = await sidenotes(browser, file);
    const bk = await book(browser, file);
    const clips = await phoneClips(browser, file);
    const snd = await pageSound(browser, file);
    console.log(
      `${file}: overflow 320px ${narrow}px, 390px ${phone}px; theme ${theme.chosen[0]}; ` +
        `contents ${toc.before}px -> ${toc.after[0]}px; book ${bk.state.pages} pages, ${bk.first} -> ${bk.second} -> ${bk.third}, ` +
        `after translation ${bk.translated.pages} pages`
    );
    const errors = theme.errors.concat(toc.errors, side.errors, bk.errors, snd.errors);
    if (snd.on !== 1 || snd.off !== 0) fail(file, `page sound: ${snd.on} plays with the sound on, ${snd.off} with it off`);
    if (side.above.length) fail(file, `footnotes above their mark: ${side.above.join(', ')}`);
    if (errors.length) fail(file, 'page errors: ' + errors.join('; '));
    if (narrow > 0 || phone > 0) fail(file, `horizontal page scroll (${narrow}px at 320, ${phone}px at 390)`);
    if (theme.darkInk !== 'rgb(22, 22, 20)') fail(file, `system dark mode not followed (${theme.darkInk})`);
    if (theme.chosen[0] !== 'white' || theme.chosen[1] !== 'rgb(255, 255, 255)' || theme.chosen[2] !== 'true')
      fail(file, `chosen theme not kept: ${theme.chosen}`);
    if (!(toc.after[0] > toc.before) || toc.after[1] !== 'false' || toc.kept !== 'hidden')
      fail(file, `contents toggle: ${JSON.stringify(toc)}`);
    if (!toc.drawer || !toc.closed) fail(file, 'contents drawer did not open and close on a phone');
    if (!bk.state.active || bk.state.pages < 2) fail(file, `book view not laid out: ${JSON.stringify(bk.state)}`);
    if (bk.unloaded) fail(file, `${bk.unloaded} images not loaded in book view`);
    if (bk.fit[0] !== bk.fit[1]) fail(file, `book page count ${bk.fit[1]} does not match the text (${bk.fit[0]} columns)`);
    if (bk.layout.apart.length) fail(file, `caption apart from its figure or table: ${bk.layout.apart.join(', ')}`);
    if (bk.layout.orphans.length) fail(file, `heading at the foot of a page: ${bk.layout.orphans.join(', ')}`);
    if (bk.layout.empty.length) fail(file, `pages more than 60% empty: ${bk.layout.empty.join(', ')}`);
    for (const [where, c] of [['phone', clips.scroll], ['phone book', clips.book], ['book', bk.layout.clips]]) {
      if (c.clipped.length) fail(file, `${where}: wide content cut off without a scroll box: ${c.clipped.join(', ')}`);
      if (c.uncued.length) fail(file, `${where}: scroll box without a fade at its cut edge: ${c.uncued.join(', ')}`);
      if (c.numbers.length) fail(file, `${where}: equation number not beside its picture: ${c.numbers.join(', ')}`);
      if (c.small.length) fail(file, `${where}: equation or table picture below 70%: ${c.small.join(', ')}`);
    }
    if (!bk.midTurn) fail(file, 'the page turn is not animated');
    if (bk.first === bk.second || bk.second === bk.third) fail(file, `pages do not turn: ${bk.first}, ${bk.second}, ${bk.third}`);
    if (!(bk.translated.pages > bk.state.pages)) fail(file, 'longer text was not paginated again');
    if (bk.back[0] !== null || !bk.back[1]) fail(file, 'the article did not return to the scroll view');
  }
  await browser.close();
  if (problems.length) console.error(`${problems.length} problem(s)`);
})();
