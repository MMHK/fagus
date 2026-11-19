#!/usr/bin/env bash
# Makes the keys and certificates in this directory, FOR TESTS ONLY: a self-signed test CA, two
# signers it certifies (RSA 2048 and ECDSA P-256), a time-stamping authority for timestamps
# (RFC 3161) and PKCS#12 files of the signers with the password `fagus-test-only`. Every private
# key here is public; nothing signed with them proves anything.
#
# Usage: tests/signing/make.sh (needs OpenSSL 3 or later). The files are committed; run it again
# only to replace them, for example before they expire in 2126.
set -euo pipefail
cd "$(dirname "$0")"
days=36500
password=fagus-test-only

openssl req -x509 -newkey rsa:2048 -noenc -keyout ca.key -out ca.pem -days $days \
  -subj "/O=Fagus tests/CN=Fagus Test CA (test only)" \
  -addext "basicConstraints=critical,CA:TRUE" -addext "keyUsage=critical,keyCertSign,cRLSign"

signer() { # name key-options subject extensions
  openssl req -newkey "$2" -noenc -keyout "$1.key" -out "$1.csr" -subj "$3"
  openssl x509 -req -in "$1.csr" -CA ca.pem -CAkey ca.key -days $days -out "$1.pem" \
    -extfile <(printf '%s\n' "$4")
  rm "$1.csr"
}
signer rsa rsa:2048 "/O=Fagus tests/CN=Fagus Test Signer RSA (test only)" \
  "basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature,nonRepudiation"
openssl ecparam -name prime256v1 -out p256.param
signer ecdsa ec:p256.param "/O=Fagus tests/CN=Fagus Test Signer ECDSA (test only)" \
  "basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature,nonRepudiation"
rm p256.param
signer tsa rsa:2048 "/O=Fagus tests/CN=Fagus Test Time-Stamping Authority (test only)" \
  "basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature
extendedKeyUsage=critical,timeStamping"

for name in rsa ecdsa; do
  openssl pkcs12 -export -inkey "$name.key" -in "$name.pem" -certfile ca.pem \
    -passout "pass:$password" -out "$name.p12"
done
# A file in the older encryption (3DES and RC2) that many programs still write.
openssl pkcs12 -export -legacy -inkey rsa.key -in rsa.pem -certfile ca.pem \
  -passout "pass:$password" -out rsa-legacy.p12
