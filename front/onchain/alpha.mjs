import { node } from './battle-ui.mjs';
import { terra } from './transactions.mjs';
export function createAlphaOnboarding({ config, wallet, watchWallet }) {
  if (!config.alpha) return { update() {} };
  const banner = document.getElementById('alpha-notice'), dialog = document.getElementById('alpha-faucet-dialog'), input = document.getElementById('alpha-faucet-wallet'), status = document.getElementById('alpha-faucet-status');
  banner.hidden = false;
  document.getElementById('open-alpha-faucet').addEventListener('click', () => { if (wallet.address) input.value = wallet.address; dialog.showModal(); });
  document.getElementById('close-alpha-faucet').addEventListener('click', () => dialog.close());
  document.getElementById('alpha-own-wallet').addEventListener('click', () => { if (wallet.address) input.value = wallet.address; });
  document.getElementById('alpha-faucet-form').addEventListener('submit', async event => {
    event.preventDefault(); const button = document.getElementById('claim-alpha-funds');
    try {
      const address = terra(input.value.trim()); button.disabled = true; status.textContent = 'Sending the test kit. Waiting for chain confirmation…';
      const response = await fetch(config.faucet, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ address }) });
      const result = await response.json(); if (!response.ok) throw Error(result.error ?? 'Faucet unavailable');
      status.replaceChildren(node('strong', 'Test kit confirmed.'), node('p', `Transaction ${result.txhash}`));
      await watchWallet(address);
    } catch (error) { status.textContent = error.message; }
    finally { button.disabled = false; }
  });
  return { update() {
    document.getElementById('alpha-own-wallet').disabled = !wallet.address;
  } };
}
