#!/usr/bin/env bash
# Create a self-signed certificate for signing vimpdf on this Mac. Run once.
#
# macOS remembers privacy permissions (like access to the Desktop) per app
# identity. An ad-hoc signature's identity is the hash of that exact build, so
# every reinstall counts as a new app and asks again. Signing with the same
# certificate each time keeps the identity. bundle-macos.sh uses this
# certificate when it finds it.
#
# The private key stays in the login keychain. To undo, delete the certificate
# named below in Keychain Access.
set -euo pipefail

name="vimpdf local signing"
keychain="$HOME/Library/Keychains/login.keychain-db"

if security find-identity -v -p codesigning | grep -qF "\"$name\""; then
    echo "certificate \"$name\" is already set up"
    exit 0
fi
if security find-certificate -c "$name" "$keychain" >/dev/null 2>&1; then
    echo "\"$name\" exists but can't sign: remove it in Keychain Access and run this again" >&2
    exit 1
fi

dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT
password=$(/usr/bin/openssl rand -hex 16)

# The system's openssl: Homebrew's writes .p12 files `security` can't read.
/usr/bin/openssl req -x509 -newkey rsa:2048 -nodes -days 3650 \
    -keyout "$dir/key.pem" -out "$dir/cert.pem" -subj "/CN=$name" \
    -addext "basicConstraints=critical,CA:false" \
    -addext "keyUsage=critical,digitalSignature" \
    -addext "extendedKeyUsage=critical,codeSigning" 2>/dev/null
/usr/bin/openssl pkcs12 -export -inkey "$dir/key.pem" -in "$dir/cert.pem" \
    -out "$dir/identity.p12" -passout "pass:$password"

security import "$dir/identity.p12" -k "$keychain" -P "$password" \
    -T /usr/bin/codesign >/dev/null
# Trusted for code signing only, for this user only. macOS asks for a password.
security add-trusted-cert -r trustRoot -p codeSign -k "$keychain" "$dir/cert.pem"

echo "created certificate \"$name\""
