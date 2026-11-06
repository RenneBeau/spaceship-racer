// Build small, self-hosted game textures from the existing artwork.
import { readFileSync, writeFileSync } from 'node:fs';
import { chromium } from '@playwright/test';
const root = new URL('../front/ui-assets/', import.meta.url);
const browser = await chromium.launch({ args: ['--no-sandbox'] });
try {
  const page = await browser.newPage();
  for (const [source, target, width, height, crop] of [
    ['background.png', 'scene-nebula.webp', 1536, 1024],
    ['hangar-background.png', 'scene-hangar.webp', 1536, 1024],
    ['commander-background.png', 'scene-commanders.webp', 1536, 1024],
    ['speed.png', 'ability-speed.webp', 192, 160, [80, 55, 360, 290]],
    ['acceleration.png', 'ability-acceleration.webp', 192, 160, [80, 55, 360, 290]],
    ['future.png', 'ability-warp.webp', 192, 160, [80, 55, 360, 290]],
    ['rocket.png', 'ability-rocket.webp', 192, 160, [80, 55, 360, 290]],
    ['shield.png', 'ability-repair.webp', 160, 160],
  ]) {
    const image = readFileSync(new URL(source, root)).toString('base64');
    const data = await page.evaluate(async ({ image, width, height, crop }) => {
      const img = new Image(); img.src = `data:image/png;base64,${image}`; await img.decode();
      const canvas = document.createElement('canvas'); canvas.width = width; canvas.height = height;
      canvas.getContext('2d').drawImage(img, ...(crop ?? [0, 0, img.width, img.height]), 0, 0, width, height);
      return canvas.toDataURL('image/webp', .86).split(',')[1];
    }, { image, width, height, crop });
    const bytes = Buffer.from(data, 'base64'); writeFileSync(new URL(target, root), bytes);
    console.log(`${target}: ${bytes.length} bytes`);
  }
} finally { await browser.close(); }
for (const [url, name] of [
  ['https://fonts.gstatic.com/s/rajdhani/v17/LDI2apCSOBg7S-QT7pbYF_Oreec.woff2', 'rajdhani-semibold.woff2'],
  ['https://fonts.gstatic.com/s/rajdhani/v17/LDI2apCSOBg7S-QT7pa8FvOreec.woff2', 'rajdhani-bold.woff2'],
]) {
  const response = await fetch(url); if (!response.ok) throw Error(`Font download failed: ${response.status}`);
  writeFileSync(new URL(name, root), Buffer.from(await response.arrayBuffer()));
}
