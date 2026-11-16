// node chrome.js <document.html> <repeats>
//
// Chrome kept running between documents, as Gotenberg or a browser pool does: one browser, a new
// tab per document. Prints JSON like bench.php and writes out/chrome-<document>.pdf.
const puppeteer = require('puppeteer');
const fs = require('fs');
const path = require('path');

(async () => {
  const [file, repeats] = process.argv.slice(2);
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
  const times = [];
  for (let i = 0; i < Number(repeats); i++) {
    start = process.hrtime.bigint();
    pdf = await render();
    times.push(Number(process.hrtime.bigint() - start) / 1e6);
  }
  times.sort((a, b) => a - b);
  await browser.close();
  fs.mkdirSync(path.join(__dirname, 'out'), { recursive: true });
  fs.writeFileSync(path.join(__dirname, 'out', 'chrome-' + path.basename(file, '.html') + '.pdf'), pdf);
  const median = times.length ? +times[Math.floor(times.length / 2)].toFixed(1) : null;
  console.log(JSON.stringify({ first: +first.toFixed(1), repeat: median }));
})();
