import { decodeTxRaw } from '@cosmjs/proto-signing';
import { sha256, ripemd160 } from '@cosmjs/crypto';
import { toBech32 } from '@cosmjs/encoding';
import { PubKey } from 'cosmjs-types/cosmos/crypto/secp256k1/keys';
import { MsgExecuteContract } from 'cosmjs-types/cosmwasm/wasm/v1/tx';
import { MsgSend } from 'cosmjs-types/cosmos/bank/v1beta1/tx';
import { SimulateRequest } from 'cosmjs-types/cosmos/tx/v1beta1/service';
import { address } from '../onchain/config.mjs';

export class AlphaPolicyError extends Error { constructor(message) { super(message); this.status = 403; } }
const reject = message => { throw new AlphaPolicyError(message); };
export function validateAlphaTransaction(bytes, policy, simulation = false) {
  if (bytes.length > 128 * 1024) reject('Transaction exceeds alpha size limit');
  let decoded;
  try { decoded = decodeTxRaw(bytes); } catch { reject('Invalid transaction encoding'); }
  return validateDecoded(decoded, policy, simulation);
}
function validateDecoded(tx, policy, simulation) {
  const info = tx.authInfo, body = tx.body;
  if (!body || !info || info.signerInfos.length !== 1 || !body.messages.length || body.messages.length > 40) reject('Alpha requires a single-signer game transaction');
  if ((body.extensionOptions?.length ?? 0) || (body.nonCriticalExtensionOptions?.length ?? 0)) reject('Transaction extensions are not supported');
  const signer = info.signerInfos[0];
  if (signer.publicKey?.typeUrl !== '/cosmos.crypto.secp256k1.PubKey') reject('A secp256k1 wallet is required');
  let sender;
  try { const key = PubKey.decode(signer.publicKey.value).key; if (key.length !== 33) reject('Invalid public key'); sender = toBech32('terra', ripemd160(sha256(key))); }
  catch { reject('Invalid wallet public key'); }
  if (policy.protectedAddresses.has(sender)) reject('Operator transactions use private infrastructure');
  if (info.fee?.granter || (info.fee?.payer && info.fee.payer !== sender)) reject('Third-party fee authorization is not supported');
  if (!info.fee || info.fee.gasLimit > 100000000n) reject('Transaction gas exceeds the alpha block limit');
  if (!simulation && (tx.signatures.length !== 1 || tx.signatures[0].length !== 64)) reject('Invalid signature encoding');
  for (const message of body.messages) {
    if (message.typeUrl === '/cosmwasm.wasm.v1.MsgExecuteContract') {
      const execute = MsgExecuteContract.decode(message.value);
      if (execute.sender !== sender || !policy.contracts.has(execute.contract)) reject('Only alpha game contracts may be executed');
      if (execute.msg.length > 32 * 1024) reject('Game action exceeds alpha size limit');
      let action; try { action = JSON.parse(Buffer.from(execute.msg).toString()); } catch { reject('Invalid game message'); }
      if (!action || typeof action !== 'object' || Array.isArray(action)) reject('Invalid game action');
      if (action.send?.contract && !policy.contracts.has(action.send.contract)) reject('Token sends must target alpha contracts');
      if (action.send_nft?.contract && !policy.contracts.has(action.send_nft.contract)) reject('NFT sends must target alpha contracts');
    } else if (message.typeUrl === '/cosmos.bank.v1beta1.MsgSend') {
      const send = MsgSend.decode(message.value);
      if (send.fromAddress !== sender || !send.amount.every(coin => ['uluna', 'uusd'].includes(coin.denom))) reject('Only test-coin transfers are supported');
      address(send.toAddress);
    } else reject('Code uploads, new contracts, migrations, governance and authorization grants are not available through the alpha gateway');
  }
  return sender;
}
export function validateSimulation(data, policy) {
  let request; try { request = SimulateRequest.decode(data); } catch { reject('Invalid simulation'); }
  if (request.txBytes?.length) return validateAlphaTransaction(request.txBytes, policy, true);
  if (request.tx) return validateDecoded(request.tx, policy, true);
  reject('A transaction is required for simulation');
}
export const READ_RPC_METHODS = new Set(['status', 'health', 'abci_info', 'block', 'block_results', 'commit', 'validators', 'tx', 'tx_search', 'block_search', 'num_unconfirmed_txs']);
export function validateRpcRequest(request, policy) {
  if (!request || request.jsonrpc !== '2.0' || typeof request.method !== 'string' || (request.params != null && typeof request.params !== 'object')) reject('Invalid JSON-RPC request');
  request.params ??= {};
  if (request.method === 'broadcast_tx_sync' || request.method === 'broadcast_tx_async') {
    if (typeof request.params.tx !== 'string' || !/^[A-Za-z0-9+/]+={0,2}$/.test(request.params.tx)) reject('Base64 transaction required');
    validateAlphaTransaction(Buffer.from(request.params.tx, 'base64'), policy); return;
  }
  if (request.method === 'abci_query') {
    const path = request.params.path;
    if (typeof path !== 'string' || !/^\/(?:cosmos\.|cosmwasm\.|terra\.|ibc\.|store\/)/.test(path)) reject('Unsupported query path');
    if (path.startsWith('/cosmos.tx.v1beta1.Service/') && !['Simulate', 'GetTx', 'GetTxsEvent', 'GetBlockWithTxs'].includes(path.split('/').at(-1))) reject('Transaction broadcasting is only supported through validated broadcast requests');
    if (path.toLowerCase().includes('simulate')) {
      if (path !== '/cosmos.tx.v1beta1.Service/Simulate' || typeof request.params.data !== 'string' || !/^(?:0x)?[0-9a-f]*$/i.test(request.params.data)) reject('Unsupported simulation format');
      validateSimulation(Buffer.from(request.params.data.replace(/^0x/, ''), 'hex'), policy);
    }
    return;
  }
  if (!READ_RPC_METHODS.has(request.method)) reject('RPC method is not exposed by the alpha gateway');
}
