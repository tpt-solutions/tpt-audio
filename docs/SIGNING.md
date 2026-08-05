# Code Signing Plan (Windows)

Distribution of `tpt-audio` on Windows should be done via a signed MSI (WiX) and,
where applicable, a signed executable. Signing builds user trust and avoids SmartScreen
warnings on first run.

## 1. Certificate

- Acquire an **Extended Validation (EV) code-signing certificate** from a trusted CA
  (e.g. DigiCert, Sectigo, GlobalSign). EV certificates immediately build SmartScreen
  reputation.
- Store the certificate in a **Hardware Security Module (HSM)** or Azure Key Vault.
  Never commit `.pfx`/private keys to the repository.
- For CI signing, use a cloud HSM / Key Vault so the private key never leaves the vault.

## 2. Signing the binary

Use `signtool.exe` (from the Windows SDK) with timestamping:

```powershell
signtool sign `
  /fd SHA256 `
  /tr http://timestamp.digicert.com `
  /td SHA256 `
  /kc <key-vault-or-hsm-reference> `
  /n "TPT Solutions" `
  target\release\tpt-audio-desktop.exe
```

## 3. Signing the installer

Sign the built MSI after `cargo wix` completes:

```powershell
signtool sign `
  /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 `
  /kc <reference> /n "TPT Solutions" `
  target\release\tpt-audio.msi
```

Sign both the inner `.exe` **and** the `.msi` — Windows validates the MSI wrapper and
the payload.

## 4. CI integration

- Keep signing material in repository **secrets** / Key Vault; inject at build time.
- Add a dedicated `release` CI job that builds in release mode, signs the binary and
  MSI, and uploads the artifacts.
- Verify signatures in CI (e.g. `signtool verify /pa target\release\tpt-audio-desktop.exe`).

## 5. Dual license

The project is dual-licensed MIT / Apache-2.0. Both `LICENSE-MIT` and `LICENSE-APACHE`
ship with the installer (see `wix/main.wxs`).

## 6. Future: Linux / Archon

- Linux packages are verified via GPG-signed tags and distro signing (see
  `packaging/linux`).
- Archon distribution is handled by the Archon package manager / capability model and
  follows that platform's signing requirements.
