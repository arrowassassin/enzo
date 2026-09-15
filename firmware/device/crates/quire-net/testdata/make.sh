#!/bin/sh
# Regenerates the TLS test fixtures with OpenSSL: a private test CA (never trusted by
# the firmware), an RSA intermediate, leaves with P-256 / RSA keys, a P-384 root, plus
# raw RSA and ECDSA signature vectors for proto/rsa.rs and proto/x509.rs. Leaves are
# valid for one year from the day this runs; the tests pin their clock to a date inside
# that year (`NOW` in x509.rs), so rerunning this script means updating `NOW`.
set -e
cd "$(dirname "$0")"
tmp=$(mktemp -d)
ext() { printf 'basicConstraints=critical,CA:%s\nsubjectAltName=%s\n' "$1" "$2" > "$tmp/ext"; }

openssl genrsa -out $tmp/ca.key 2048 2>/dev/null
openssl req -x509 -new -key $tmp/ca.key -sha256 -days 3650 -subj "/O=Quire Test/CN=Quire Test Root RSA" -out $tmp/ca.pem
openssl genrsa -out $tmp/inter.key 2048 2>/dev/null
openssl req -new -key $tmp/inter.key -subj "/O=Quire Test/CN=Quire Test Intermediate" -out $tmp/inter.csr
ext true DNS:inter.example.test
openssl x509 -req -in $tmp/inter.csr -CA $tmp/ca.pem -CAkey $tmp/ca.key -CAcreateserial -sha256 -days 3650 -extfile $tmp/ext -out $tmp/inter.pem 2>/dev/null

openssl ecparam -name prime256v1 -genkey -noout -out $tmp/leaf.key
openssl req -new -key $tmp/leaf.key -subj "/CN=books.example.test" -out $tmp/leaf.csr
ext false "DNS:books.example.test,DNS:*.example.test"
openssl x509 -req -in $tmp/leaf.csr -CA $tmp/inter.pem -CAkey $tmp/inter.key -CAcreateserial -sha256 -days 365 -extfile $tmp/ext -out $tmp/leaf.pem 2>/dev/null

openssl genrsa -out $tmp/leafrsa.key 2048 2>/dev/null
openssl req -new -key $tmp/leafrsa.key -subj "/CN=rsa.example.test" -out $tmp/leafrsa.csr
ext false DNS:rsa.example.test
openssl x509 -req -in $tmp/leafrsa.csr -CA $tmp/inter.pem -CAkey $tmp/inter.key -CAcreateserial -sha256 -days 365 -extfile $tmp/ext -out $tmp/leafrsa.pem 2>/dev/null

openssl ecparam -name secp384r1 -genkey -noout -out $tmp/ca384.key
openssl req -x509 -new -key $tmp/ca384.key -sha384 -days 3650 -subj "/O=Quire Test/CN=Quire Test Root P-384" -out $tmp/ca384.pem
openssl ecparam -name prime256v1 -genkey -noout -out $tmp/leaf384.key
openssl req -new -key $tmp/leaf384.key -subj "/CN=ecc.example.test" -out $tmp/leaf384.csr
ext false DNS:ecc.example.test
openssl x509 -req -in $tmp/leaf384.csr -CA $tmp/ca384.pem -CAkey $tmp/ca384.key -CAcreateserial -sha384 -days 365 -extfile $tmp/ext -out $tmp/leaf384.pem 2>/dev/null

for p in ca:ca-rsa inter:inter-rsa leaf:leaf-p256 leafrsa:leaf-rsa ca384:ca-p384 leaf384:leaf-p384-chain; do
  openssl x509 -in $tmp/${p%%:*}.pem -outform DER -out ${p##*:}.der
done
cat $tmp/ca.pem $tmp/ca384.pem > $tmp/roots.pem
cp ../roots/build.py $tmp/build.py && (cd $tmp && python3 build.py > /dev/null) && cp $tmp/roots.bin roots.bin

# Raw RSA vectors (the leaf's RSA key): PKCS#1 v1.5 and PSS over a fixed message.
printf 'quire rsa test message' > $tmp/msg
openssl rsa -in $tmp/leafrsa.key -modulus -noout 2>/dev/null | sed 's/Modulus=//' | python3 -c 'import sys;sys.stdout.buffer.write(bytes.fromhex(sys.stdin.read().strip()))' > rsa2048-n.bin
openssl dgst -sha256 -sign $tmp/leafrsa.key -out rsa2048-sig-pkcs1-sha256.bin $tmp/msg
openssl dgst -sha256 -sigopt rsa_padding_mode:pss -sigopt rsa_pss_saltlen:32 -sign $tmp/leafrsa.key -out rsa2048-sig-pss-sha256.bin $tmp/msg

# TLS 1.3 CertificateVerify-style vectors: a 130-byte message signed by each leaf.
head -c 130 /dev/urandom > tls-msg.bin
openssl dgst -sha256 -sigopt rsa_padding_mode:pss -sigopt rsa_pss_saltlen:32 -sign $tmp/leafrsa.key -out tls-sig-rsa-pss-sha256.bin tls-msg.bin
openssl dgst -sha256 -sign $tmp/leaf.key -out tls-sig-p256-sha256.bin tls-msg.bin
rm -rf $tmp
echo "fixtures written; leaves valid from $(date -u +%Y-%m-%d) for 365 days"
