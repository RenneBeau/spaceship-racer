import { existsSync, mkdirSync } from 'node:fs';
import { resolve } from 'node:path';
import { execFileSync } from 'node:child_process';
const revision = '8e865d3721231189cbad1730c7a24698038c9cfb';
const folder = resolve('vendor/cl8y-dex-terraclassic');
mkdirSync('vendor', { recursive: true });
if (!existsSync(folder)) {
  execFileSync('git', ['clone', 'https://github.com/PlasticDigits/cl8y-dex-terraclassic.git', folder], { stdio: 'inherit' });
  execFileSync('git', ['-C', folder, 'checkout', '--detach', revision], { stdio: 'inherit' });
}
const actual = execFileSync('git', ['-C', folder, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim();
if (actual !== revision) throw new Error(`Existing CL8Y checkout is ${actual}; expected ${revision}. No files were overwritten.`);
console.log(`CL8Y source ready at ${folder} (${revision})`);
