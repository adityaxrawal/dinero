# ⚠️ `license_public.pem` is a placeholder — replace before release

`license_public.pem` is a throwaway RSA-2048 public key generated locally for
development and testing only (Doc 22 §10.2). Its matching private key was
never persisted anywhere — it existed only for the seconds it took `openssl`
to derive this public key, then was discarded. No JWT signed by any real key
will verify against it, by design.

Before release, replace `license_public.pem` with the real RSA-256 public key
corresponding to the Licensing Backend's production signing key (held as a
Vercel encrypted environment secret, Doc 22 §10.2/§10.4). This file is not a
secret itself — only the private key is — so committing the real public key
here at release time is correct and expected, matching the documented
`include_str!("keys/license_public.pem")` embedding model.
