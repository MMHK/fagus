// node chrome.js <document.html> <repeats> [marked|free] [tabs]
//
// chrome.js of ../compare (one Chrome kept running, a new tab per document) with the phase
// markers of engine.php. With tabs > 1, each of the <repeats> rounds renders in that many tabs at
// once. Uses the Puppeteer installed in ../compare.
const path = require('path');
const fs = require('fs');
const readline = require('readline');
const puppeteer = require(require.resolve('puppeteer', { paths: [path.join(__dirname, '..', 'compare')] }));

const lines = readline.createInterface({ input: process.stdin });
const queue = [];
let waiter = null;
lines.on('line', (l) => { if (waiter) { const w = waiter; waiter = null; w(l); } else queue.push(l); });
const nextLine = () => queue.length ? Promise.resolve(queue.shift()) : new Promise((r) => { waiter = r; });

(async () => {
  const [file, repeats, mode = 'marked', tabs = '1'] = process.argv.slice(2);
  const marked = mode === 'marked';
  const mark = async (m) => { process.stdout.write('@@' + m + '\n'); if (marked) await nextLine(); };
  const browser = await puppeteer.launch({
    executablePath: process.env.CHROME || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
    headless: true,
  });
  const render = async () => {
    const page = await browser.newPage();
    await page.goto('file://' + path.resolve(file), { waitUntil: 'load' });
    const pdf = await page.pdf({ printBackground: true, preferCSSPageSize: true });
    await page.close();
    return pdf;
  };
  let start = process.hrtime.bigint();
  let pdf = await render();
  const first = Number(process.hrtime.bigint() - start) / 1e6;
  await mark('cold');
  const warmStart = Date.now() / 1000;
  const times = [];
  for (let i = 0; i < Number(repeats); i++) {
    start = process.hrtime.bigint();
    [pdf] = await Promise.all(Array.from({ length: Number(tabs) }, render));
    times.push(Number(process.hrtime.bigint() - start) / 1e6);
  }
  const warmEnd = Date.now() / 1000;
  await mark('warm');
  times.sort((a, b) => a - b);
  await browser.close();
  lines.close();
  fs.mkdirSync(path.join(__dirname, 'out'), { recursive: true });
  fs.writeFileSync(path.join(__dirname, 'out', 'chrome-' + path.basename(file, '.html') + '.pdf'), pdf);
  const median = times.length ? +times[Math.floor(times.length / 2)].toFixed(1) : null;
  const mean = times.length ? +(times.reduce((a, b) => a + b, 0) / times.length).toFixed(1) : null;
  console.log(JSON.stringify({ first: +first.toFixed(1), median, mean, n: times.length * Number(tabs), warm_start: warmStart, warm_end: warmEnd }));
  process.exit(0);
})();
