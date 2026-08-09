// Doc 30 TASK-LIC-005: one-time (or key-rotation-time) RS256 keypair
// generation. Run manually: `npx tsx scripts/generate_keypair.ts`.
//
// The private key must go ONLY into Vercel's encrypted env secrets
// (JWT_PRIVATE_KEY_PEM) -- never committed. The public key gets pasted
// into src-tauri/keys/license_public.pem and compiled into the Tauri
// binary via include_str!, so the desktop app verifies JWT signatures
// entirely offline (Doc 15 hybrid model).
//
// Key-rotation runbook (Doc 30 TASK-LIC-005, documented not automated for v1):
//   1. Run this script to generate a new keypair.
//   2. Set the new private key as Vercel's JWT_PRIVATE_KEY_PEM (rotate the old
//      one out of the secret store once the dual-verification window below closes).
//   3. Ship a desktop app update embedding the new public key (TASK-DESK-005
//      auto-updater) alongside the OLD public key too, so validate()/jwt
//      verification in src-tauri/src/licensing/jwt.rs accepts either signature
//      for a brief window -- avoids locking out users who haven't updated yet.
//   4. Once telemetry shows negligible traffic still verifying against the old
//      key, remove it from the desktop verification allowlist in the next release.
import { generateKeyPairSync } from 'node:crypto';

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
