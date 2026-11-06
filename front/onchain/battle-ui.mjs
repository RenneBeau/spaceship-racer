import { display } from './transactions.mjs';
export const node = (tag, text, cls) => { const e = document.createElement(tag); if (text !== undefined) e.textContent = text; if (cls) e.className = cls; return e; };
export const units = value => Number(display(String(value ?? '0'), 3)).toLocaleString(undefined, { maximumFractionDigits: 1 });
export function ratio(value, maximum) { return BigInt(maximum ?? '0') > 0n ? Math.min(100, Number(BigInt(value ?? '0') * 10000n / BigInt(maximum)) / 100) : 0; }
export function healthBar(label, value, maximum, shield = false) {
  const box = node('div', undefined, shield ? 'vital-bar shield' : 'vital-bar hp');
  box.dataset.current = String(value); box.dataset.maximum = String(maximum);
  if (!shield && ratio(value, maximum) < 25) box.classList.add('critical');
  const heading = node('div', undefined, 'vital-label'); heading.append(node('span', label), node('strong', `${units(value)} / ${units(maximum)}`));
  const track = node('div', undefined, 'vital-track'), fill = node('span'); fill.style.width = `${ratio(value, maximum)}%`; track.append(fill);
  track.setAttribute('role', 'progressbar'); track.setAttribute('aria-label', label); track.setAttribute('aria-valuemin', '0'); track.setAttribute('aria-valuemax', '100'); track.setAttribute('aria-valuenow', String(ratio(value, maximum)));
  box.append(heading, track); return box;
}
