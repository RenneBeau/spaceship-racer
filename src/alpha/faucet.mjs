import Database from 'better-sqlite3';
import { createHash, randomUUID } from 'node:crypto';
import { chmodSync } from 'node:fs';
import { execFile } from 'node:child_process';
import { address } from '../onchain/config.mjs';
import { fromBech32 } from '@cosmjs/encoding';
export class FaucetError extends Error { constructor(status, message) { super(message); this.status = status; } }
export function createFaucet({ manifest, filename, protectedAddresses, submit = null, now = Date.now }) {
  if (manifest.chain_id !== 'spaceship-racer-alpha') throw Error('Faucet is restricted to the dedicated alpha chain');
  const db = new Database(filename); if (filename !== ':memory:') chmodSync(filename, 0o600);
  db.pragma('journal_mode = WAL');
  db.exec('CREATE TABLE IF NOT EXISTS grants(id TEXT PRIMARY KEY,address TEXT NOT NULL,ip TEXT NOT NULL,created INTEGER NOT NULL,status TEXT NOT NULL,txhash TEXT,error TEXT); CREATE INDEX IF NOT EXISTS grant_address ON grants(address,created); CREATE INDEX IF NOT EXISTS grant_ip ON grants(ip,created)');
  let queue = Promise.resolve();
  const assets = manifest.paymentTokens;
  const quantities = assets.map(token => ({ token, amount: token === manifest.srt ? '10000000000' : '1000000000' }));
  const reserve = db.transaction((wallet, ip) => {
    const since = now() - 86400000, hash = createHash('sha256').update(ip).digest('hex');
    const previous = db.prepare("SELECT created FROM grants WHERE address=? AND created>? AND status!='failed' ORDER BY created DESC LIMIT 1").get(wallet, since);
    if (previous) throw new FaucetError(429, 'This wallet already received a test kit in the last 24 hours');
    if (db.prepare("SELECT COUNT(*) AS n FROM grants WHERE ip=? AND created>? AND status!='failed'").get(hash, since).n >= 3) throw new FaucetError(429, 'This connection has reached the daily test-kit limit');
    if (db.prepare("SELECT COUNT(*) AS n FROM grants WHERE created>? AND status!='failed'").get(since).n >= 100) throw new FaucetError(429, 'The daily faucet budget has been reached');
    const id = randomUUID(); db.prepare('INSERT INTO grants(id,address,ip,created,status) VALUES(?,?,?,?,?)').run(id, wallet, hash, now(), 'pending'); return id;
  });
  async function broadcast(wallet) {
    const messages = [{ '@type': '/cosmos.bank.v1beta1.MsgSend', from_address: manifest.faucet_address, to_address: wallet, amount: [{ denom: 'uluna', amount: '1000000000000' }] },
      ...quantities.map(({ token, amount }) => ({ '@type': '/cosmwasm.wasm.v1.MsgExecuteContract', sender: manifest.faucet_address, contract: token, msg: { mint: { recipient: wallet, amount } }, funds: [] }))];
    const transaction = { body: { messages, memo: 'Spaceship Racer alpha test kit — no monetary value', timeout_height: '0', extension_options: [], non_critical_extension_options: [] }, auth_info: { signer_infos: [], fee: { amount: [{ denom: 'uusd', amount: '7500000' }], gas_limit: '10000000', payer: '', granter: '' } }, signatures: [] };
    const args = ['compose', '-f', manifest.compose, 'exec', '-T', 'chain', 'terrad'];
    // execFile has no stdin option; a short-lived child receives only the fixed
    // transaction JSON. No key material leaves the alpha Docker keyring.
    const command = (rest, input) => new Promise((resolve, reject) => {
      const child = execFile('docker', [...args, ...rest], { maxBuffer: 2 * 1024 * 1024 }, (error, stdout) => error ? reject(error) : resolve(stdout));
      if (input) child.stdin.end(input); else child.stdin.end();
    });
    const signed = await command(['tx', 'sign', '/dev/stdin', '--from', manifest.faucet_key, '--keyring-backend', 'test', '--chain-id', manifest.chain_id, '--output', 'json'], JSON.stringify(transaction));
    const sent = JSON.parse(await command(['tx', 'broadcast', '/dev/stdin', '--broadcast-mode', 'sync', '--output', 'json'], signed));
    if (Number(sent.code ?? 0)) throw Object.assign(Error('Faucet transaction rejected'), { definitive: true });
    for (let n = 0; n < 120; n++) {
      const response = await fetch(`${manifest.indexer.rpc}/tx?hash=0x${sent.txhash}`, { signal: AbortSignal.timeout(5000) });
      const result = (await response.json()).result;
      if (result) {
        if (Number(result.tx_result.code ?? 0)) throw Object.assign(Error('Faucet execution failed'), { definitive: true });
        return { txhash: sent.txhash, height: Number(result.height) };
      }
      await new Promise(resolve => setTimeout(resolve, 500));
    }
    throw Error('Faucet confirmation is pending');
  }
  const kit = { nativeLunc: '1000000', assets: quantities, cooldownHours: 24 };
  return { db, kit,
    async claim(wallet, ip, { firstLogin = false } = {}) {
      address(wallet); if (fromBech32(wallet).data.length !== 20) throw new FaucetError(400, 'Use a wallet address, not a contract address');
      if (protectedAddresses.has(wallet)) throw new FaucetError(403, 'Operator wallets are funded privately');
      if (firstLogin) {
        const previous = db.prepare("SELECT status,txhash FROM grants WHERE address=? AND status!='failed' ORDER BY created DESC LIMIT 1").get(wallet);
        if (previous?.status === 'submitted') return { address: wallet, alreadyFunded: true, txhash: previous.txhash, kit };
        if (previous) throw new FaucetError(409, 'Starter funding is pending confirmation. Please reconnect shortly.');
      }
      const id = reserve(wallet, ip);
      const operation = queue.then(async () => {
        try {
          const result = await (submit ?? broadcast)(wallet, quantities);
          db.prepare('UPDATE grants SET status=?,txhash=? WHERE id=?').run('submitted', result.txhash, id);
          return { id, address: wallet, ...result, kit };
        } catch (error) {
          // Unknown broadcast outcome remains reserved to avoid duplicate grants.
          db.prepare('UPDATE grants SET status=?,error=? WHERE id=?').run(error.definitive ? 'failed' : 'pending_review', String(error.message).slice(0, 200), id);
          throw new FaucetError(503, 'Test-kit funding is temporarily unavailable. Please retry later.');
        }
      });
      queue = operation.catch(() => {}); return operation;
    },
    close() { db.close(); },
  };
}
