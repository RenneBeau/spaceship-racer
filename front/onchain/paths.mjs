export function baseFromPath(pathname) { return /^\/spaceship-racer(?:\/|$)/.test(pathname) ? '/spaceship-racer' : ''; }
export const BASE_PATH = typeof location === 'undefined' ? '' : baseFromPath(location.pathname);
export function appPath(path = '/') { return `${BASE_PATH}${path.startsWith('/') ? path : `/${path}`}`; }
export function routePath() { return location.pathname.slice(BASE_PATH.length) || '/'; }
