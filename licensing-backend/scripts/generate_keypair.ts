/**
 * One-off generator for the RS256 keypair backing license tokens.
 *
 * The split is the whole point of the asymmetric scheme: the private key signs
 * tokens and lives only in the server environment, while the public key ships
 * inside the desktop app so it can verify a license offline. A client holding
 * only the public key can check authenticity but cannot mint a token.
 *
 * Keys are printed to stdout and never written to disk, so neither ends up
 * committed by accident.
 */
import { generateKeyPairSync } from 'node:crypto';

/**
 * Generates an RS256 keypair and prints both halves to stdout.
 */
function main() {
  const { publicKey, privateKey } = generateKeyPairSync('rsa', {
    modulusLength: 2048,
    publicKeyEncoding: { type: 'spki', format: 'pem' },
    privateKeyEncoding: { type: 'pkcs8', format: 'pem' },
  });

  console.log('--- PRIVATE KEY (set as Vercel env JWT_PRIVATE_KEY_PEM, never commit) ---');
  console.log(privateKey);
  console.log('--- PUBLIC KEY (paste into src-tauri/keys/license_public.pem) ---');
  console.log(publicKey);
}

main();
