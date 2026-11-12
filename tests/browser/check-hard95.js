// Browser checks of hard-95 output (the parts only a browser can test).
//
// For each file:
//  - the page scripts run without errors;
//  - flowing lines are fitted to their width in the PDF;
//  - the text of every page is in the document before any scrolling
//    (page pictures load lazily, text must not);
//  - when the text of paragraphs is replaced by a longer text, as a
//    translator does, every block reflows and nothing overflows its boxes.
//
// usage: node tests/browser/check-hard95.js file.hard-95.html [...]
// Uses the Playwright package if installed, else playwright-core with the
// system Chrome (set P2H_CHROME_CHANNEL=chrome, as CI does).
let pw;
try {
  pw = require('playwright');
} catch (e) {
  pw = require('playwright-core');
}

const channel = process.env.P2H_CHROME_CHANNEL;

function fail(file, msg) {
  console.error(`FAIL ${file}: ${msg}`);
  process.exitCode = 1;
}

(async () => {
  const files = process.argv.slice(2);
  if (!files.length) {
    console.error('usage: node check-hard95.js file.html [...]');
    process.exit(2);
  }
  const browser = await pw.chromium.launch(channel ? { channel } : {});
  for (const file of files) {
    const page = await browser.newPage({ viewport: { width: 1000, height: 900 } });
    const errors = [];
    page.on('pageerror', (e) => errors.push(e.message));
    await page.goto('file://' + require('path').resolve(file), { waitUntil: 'load' });

    const textBeforeScroll = await page.evaluate(() =>
      [...document.querySelectorAll('.page')].map((p) => p.querySelector('.tl').textContent.trim().length)
    );
    const emptyPages = textBeforeScroll.filter((n) => n === 0).length;

    await page.evaluate(() => window.paper2htmlFitAll());
    const fit = await page.evaluate(() => {
      let lines = 0;
      let misfit = 0;
      document.querySelectorAll('.page').forEach((pg) => {
        const w = pg.getBoundingClientRect().width;
        pg.querySelectorAll('.fb:not(.cont) .ln').forEach((l) => {
          lines++;
          const target = (parseFloat(l.dataset.w) / 100) * w;
          if (Math.abs(l.getBoundingClientRect().width - target) > 1.5) misfit++;
        });
      });
      return { lines, misfit };
    });

    // Simulated translation of the first two pages.
    const translated = await page.evaluate(() => {
      const filler = 'Đây là bản dịch thử nghiệm của đoạn văn, dài hơn bản gốc để kiểm tra việc dàn lại chữ. ';
      let n = 0;
      document
        .querySelectorAll(
          '#page-1 .fb:not(.cont)[data-kind="paragraph"], #page-2 .fb:not(.cont)[data-kind="paragraph"]'
        )
        .forEach((fb) => {
          const len = fb.textContent.length;
          let t = '';
          while (t.length < len * 1.15) t += filler;
          t = t.slice(0, Math.round(len * 1.15));
          const lines = fb.querySelectorAll('.ln');
          const per = Math.ceil(t.length / lines.length);
          lines.forEach((ln, i) => {
            ln.textContent = t.slice(i * per, (i + 1) * per);
          });
          n++;
        });
      return n;
    });
    await page.waitForTimeout(500);
    const reflow = await page.evaluate(() => {
      let reflowed = 0;
      let overflow = 0;
      document.querySelectorAll('.fb.rf:not(.cont)').forEach((fb) => {
        reflowed++;
        let room = fb.getBoundingClientRect().height;
        document
          .querySelectorAll('.fb.cont[data-of="' + fb.dataset.block + '"]')
          .forEach((c) => (room += c.getBoundingClientRect().height));
        if (fb.firstElementChild.scrollHeight > room + 4) overflow++;
      });
      return { reflowed, overflow };
    });

    console.log(
      `${file}: ${textBeforeScroll.length} pages, ${fit.lines} lines (${fit.misfit} not fitted), ` +
        `${translated} paragraphs translated, ${reflow.reflowed} reflowed, ${reflow.overflow} overflowing`
    );
    if (errors.length) fail(file, 'page errors: ' + errors.join('; '));
    if (emptyPages) fail(file, `${emptyPages} pages have no text before scrolling`);
    if (fit.lines === 0) fail(file, 'no flowing lines found');
    if (fit.misfit > fit.lines * 0.01) fail(file, `${fit.misfit} of ${fit.lines} lines not fitted to their width`);
    if (translated === 0) fail(file, 'no paragraph on the first two pages');
    if (reflow.reflowed < translated) fail(file, `${translated - reflow.reflowed} translated blocks did not reflow`);
    if (reflow.overflow) fail(file, `${reflow.overflow} translated blocks overflow their boxes`);
    await page.close();
  }
  await browser.close();
})();
