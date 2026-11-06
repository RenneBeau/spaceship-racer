// Read-only check for a user timer or an external supervisor.
const url = process.env.RACER_HEALTH_URL ?? 'http://127.0.0.1:5100/api/health';
try {
  const response = await fetch(url, { signal: AbortSignal.timeout(10000) }), health = await response.json();
  if (!response.ok || health.chainId !== (process.env.RACER_HEALTH_CHAIN ?? 'spaceship-racer-local') || health.healthy !== true || health.blockAgeSeconds > 30) {
    console.error(JSON.stringify({ alert: 'Spaceship chain unhealthy', status: response.status, ...health })); process.exitCode = 1;
  } else console.log(JSON.stringify({ healthy: true, height: health.height, blockAgeSeconds: health.blockAgeSeconds, activeBondedValidators: health.activeBondedValidators, indexLag: health.indexLag }));
} catch (error) { console.error(JSON.stringify({ alert: 'Spaceship health endpoint unavailable', error: error.message })); process.exitCode = 1; }
