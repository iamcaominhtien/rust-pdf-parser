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
//  - book view: pages are laid out, the buttons and keys turn them, a page
//    turn takes its time and ends, the page count covers the text once all
//    images have loaded, the article returns to the scroll view,
//    and a translated (longer) text is paginated again.
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
  return { state, fit, unloaded, first, midTurn, second, third, translated, back, errors };
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
    const bk = await book(browser, file);
    console.log(
      `${file}: overflow 320px ${narrow}px, 390px ${phone}px; theme ${theme.chosen[0]}; ` +
        `contents ${toc.before}px -> ${toc.after[0]}px; book ${bk.state.pages} pages, ${bk.first} -> ${bk.second} -> ${bk.third}, ` +
        `after translation ${bk.translated.pages} pages`
    );
    const errors = theme.errors.concat(toc.errors, bk.errors);
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
    if (!bk.midTurn) fail(file, 'the page turn is not animated');
    if (bk.first === bk.second || bk.second === bk.third) fail(file, `pages do not turn: ${bk.first}, ${bk.second}, ${bk.third}`);
    if (!(bk.translated.pages > bk.state.pages)) fail(file, 'longer text was not paginated again');
    if (bk.back[0] !== null || !bk.back[1]) fail(file, 'the article did not return to the scroll view');
  }
  await browser.close();
  if (problems.length) console.error(`${problems.length} problem(s)`);
})();
