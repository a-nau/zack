#!/usr/bin/env bash
# One-time setup: create a self-signed code-signing certificate named
# "zack-codesign" in the login keychain. scripts/install/macos.sh then signs the
# zack binary with it on every install, keeping the code signature — and
# therefore the macOS Accessibility grant — STABLE across rebuilds. Without
# this, every rebuild silently voids the grant and it must be re-added.
#
# Run this yourself (it touches your keychain): scripts/macos-sign-setup.sh
# Approve the keychain/trust prompts when macOS shows them. When codesign
# first uses the key, click "Always Allow".
set -euo pipefail

if security find-identity -v -p codesigning | grep -q zack-codesign; then
    echo "zack-codesign identity already exists — nothing to do"
    exit 0
fi

STAGE="$(mktemp -d "${TMPDIR:-/tmp}/zack-sign.XXXXXX")"
trap 'rm -rf "$STAGE"' EXIT

openssl req -x509 -newkey rsa:2048 -keyout "$STAGE/key.pem" -out "$STAGE/cert.pem" \
    -days 3650 -nodes -subj "/CN=zack-codesign" \
    -addext "basicConstraints=critical,CA:false" \
    -addext "keyUsage=critical,digitalSignature" \
    -addext "extendedKeyUsage=critical,codeSigning"
# macOS `security import` cannot read OpenSSL 3's default PKCS12 encryption;
# force the legacy SHA1/3DES algorithms it understands.
openssl pkcs12 -export -out "$STAGE/zack.p12" -inkey "$STAGE/key.pem" \
    -in "$STAGE/cert.pem" -passout pass:zack-import -name zack-codesign \
    -certpbe PBE-SHA1-3DES -keypbe PBE-SHA1-3DES -macalg sha1
security import "$STAGE/zack.p12" -k ~/Library/Keychains/login.keychain-db \
    -P zack-import -T /usr/bin/codesign
# Trust it for code signing (user domain) — macOS asks for confirmation.
security add-trusted-cert -p codeSign -k ~/Library/Keychains/login.keychain-db \
    "$STAGE/cert.pem"

echo
echo "Done. Now sign the installed binary and restart zack:"
echo "  codesign --force --sign zack-codesign --identifier com.zack.launcher ~/.local/bin/zack"
echo "  launchctl kickstart -k \"gui/\$(id -u)/com.zack.launcher\""
echo "Then re-grant Accessibility ONE last time — it survives all future rebuilds."
