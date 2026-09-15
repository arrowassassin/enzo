#!/bin/sh
# Regenerates the TLS test fixtures with OpenSSL: a private test CA (never trusted by
# the firmware), an RSA intermediate, leaves with P-256 / RSA keys, a P-384 root, plus
# raw RSA and ECDSA signature vectors for proto/rsa.rs and proto/x509.rs. Leaves are
# valid for one year from the day this runs; the tests pin their clock to a date inside
# that year (`NOW` in x509.rs), so rerunning this script means updating `NOW`.
#
# Negative fixtures for the chain walker: an intermediate with `pathlen:0` and a CA
# under it, a name-constrained intermediate, and a leaf signed by another leaf.
set -e
cd "$(dirname "$0")"
tmp=$(mktemp -d)
ext() { printf 'basicConstraints=critical,CA:%s\nsubjectAltName=%s\n' "$1" "$2" > "$tmp/ext"; }
# sign <name> <issuer> <days> <hash>: signs $tmp/<name>.csr with $tmp/ext into $tmp/<name>.pem.
sign() { openssl x509 -req -in $tmp/$1.csr -CA $tmp/$2.pem -CAkey $tmp/$2.key -CAcreateserial -$4 -days $3 -extfile $tmp/ext -out $tmp/$1.pem 2>/dev/null; }

openssl genrsa -out $tmp/ca.key 2048 2>/dev/null
openssl req -x509 -new -key $tmp/ca.key -sha256 -days 3650 -subj "/O=Quire Test/CN=Quire Test Root RSA" -out $tmp/ca.pem
openssl genrsa -out $tmp/inter.key 2048 2>/dev/null
openssl req -new -key $tmp/inter.key -subj "/O=Quire Test/CN=Quire Test Intermediate" -out $tmp/inter.csr
ext true DNS:inter.example.test
sign inter ca 3650 sha256

openssl ecparam -name prime256v1 -genkey -noout -out $tmp/leaf.key
openssl req -new -key $tmp/leaf.key -subj "/CN=books.example.test" -out $tmp/leaf.csr
ext false "DNS:books.example.test,DNS:*.example.test"
sign leaf inter 365 sha256

openssl genrsa -out $tmp/leafrsa.key 2048 2>/dev/null
openssl req -new -key $tmp/leafrsa.key -subj "/CN=rsa.example.test" -out $tmp/leafrsa.csr
ext false "DNS:rsa.example.test,IP:192.0.2.7"
sign leafrsa inter 365 sha256

openssl ecparam -name secp384r1 -genkey -noout -out $tmp/ca384.key
openssl req -x509 -new -key $tmp/ca384.key -sha384 -days 3650 -subj "/O=Quire Test/CN=Quire Test Root P-384" -out $tmp/ca384.pem
openssl ecparam -name prime256v1 -genkey -noout -out $tmp/leaf384.key
openssl req -new -key $tmp/leaf384.key -subj "/CN=ecc.example.test" -out $tmp/leaf384.csr
ext false DNS:ecc.example.test
sign leaf384 ca384 365 sha384

# pathlen:0 intermediate, a CA under it (one too many), and a leaf under each.
openssl ecparam -name prime256v1 -genkey -noout -out $tmp/pl0.key
openssl req -new -key $tmp/pl0.key -subj "/O=Quire Test/CN=Quire Test Pathlen 0" -out $tmp/pl0.csr
printf 'basicConstraints=critical,CA:true,pathlen:0\n' > $tmp/ext
sign pl0 ca 3650 sha256
openssl ecparam -name prime256v1 -genkey -noout -out $tmp/pl0sub.key
openssl req -new -key $tmp/pl0sub.key -subj "/O=Quire Test/CN=Quire Test Under Pathlen 0" -out $tmp/pl0sub.csr
printf 'basicConstraints=critical,CA:true\n' > $tmp/ext
sign pl0sub pl0 3650 sha256
openssl ecparam -name prime256v1 -genkey -noout -out $tmp/leafpl.key
openssl req -new -key $tmp/leafpl.key -subj "/CN=pl.example.test" -out $tmp/leafpl.csr
ext false DNS:pl.example.test
sign leafpl pl0 365 sha256
openssl req -new -key $tmp/leafpl.key -subj "/CN=deep.example.test" -out $tmp/leafdeep.csr
ext false DNS:deep.example.test
sign leafdeep pl0sub 365 sha256

# A name-constrained intermediate (only *.allowed.test) and a leaf naming both sides.
openssl ecparam -name prime256v1 -genkey -noout -out $tmp/nc.key
openssl req -new -key $tmp/nc.key -subj "/O=Quire Test/CN=Quire Test Constrained" -out $tmp/nc.csr
printf 'basicConstraints=critical,CA:true\nnameConstraints=critical,permitted;DNS:allowed.test,excluded;DNS:bad.allowed.test\n' > $tmp/ext
sign nc ca 3650 sha256
openssl req -new -key $tmp/leafpl.key -subj "/CN=x.allowed.test" -out $tmp/leafnc.csr
ext false "DNS:x.allowed.test,DNS:allowed.test,DNS:y.bad.allowed.test,DNS:books.example.test"
sign leafnc nc 365 sha256

# A leaf signed by the RSA leaf (CA:false).
openssl req -new -key $tmp/leafpl.key -subj "/CN=byleaf.example.test" -out $tmp/leafbyleaf.csr
ext false DNS:byleaf.example.test
sign leafbyleaf leafrsa 365 sha256

for p in ca:ca-rsa inter:inter-rsa leaf:leaf-p256 leafrsa:leaf-rsa ca384:ca-p384 leaf384:leaf-p384-chain \
         pl0:inter-pathlen0 pl0sub:inter-under-pathlen0 leafpl:leaf-pathlen0 leafdeep:leaf-under-pathlen0 \
         nc:inter-constrained leafnc:leaf-constrained leafbyleaf:leaf-by-leaf; do
  openssl x509 -in $tmp/${p%%:*}.pem -outform DER -out ${p##*:}.der
done
cat $tmp/ca.pem $tmp/ca384.pem > $tmp/roots.pem
cp ../roots/build.py $tmp/build.py && (cd $tmp && python3 build.py > /dev/null) && cp $tmp/roots.bin roots.bin

# Raw RSA vectors (the leaf's RSA key and a 4096-bit key): PKCS#1 v1.5 and PSS (salt as
# long as the digest, as TLS 1.3 requires) over a fixed message.
printf 'quire rsa test message' > $tmp/msg
modulus() { openssl rsa -in $1 -modulus -noout 2>/dev/null | sed 's/Modulus=//' | python3 -c 'import sys;sys.stdout.buffer.write(bytes.fromhex(sys.stdin.read().strip()))'; }
modulus $tmp/leafrsa.key > rsa2048-n.bin
openssl dgst -sha256 -sign $tmp/leafrsa.key -out rsa2048-sig-pkcs1-sha256.bin $tmp/msg
openssl dgst -sha384 -sign $tmp/leafrsa.key -out rsa2048-sig-pkcs1-sha384.bin $tmp/msg
openssl dgst -sha512 -sign $tmp/leafrsa.key -out rsa2048-sig-pkcs1-sha512.bin $tmp/msg
openssl dgst -sha256 -sigopt rsa_padding_mode:pss -sigopt rsa_pss_saltlen:32 -sign $tmp/leafrsa.key -out rsa2048-sig-pss-sha256.bin $tmp/msg
openssl dgst -sha384 -sigopt rsa_padding_mode:pss -sigopt rsa_pss_saltlen:48 -sign $tmp/leafrsa.key -out rsa2048-sig-pss-sha384.bin $tmp/msg
openssl dgst -sha512 -sigopt rsa_padding_mode:pss -sigopt rsa_pss_saltlen:64 -sign $tmp/leafrsa.key -out rsa2048-sig-pss-sha512.bin $tmp/msg
# A PSS signature with a salt shorter than the digest must be rejected.
openssl dgst -sha256 -sigopt rsa_padding_mode:pss -sigopt rsa_pss_saltlen:20 -sign $tmp/leafrsa.key -out rsa2048-sig-pss-sha256-salt20.bin $tmp/msg
openssl genrsa -out $tmp/rsa4096.key 4096 2>/dev/null
modulus $tmp/rsa4096.key > rsa4096-n.bin
openssl dgst -sha256 -sign $tmp/rsa4096.key -out rsa4096-sig-pkcs1-sha256.bin $tmp/msg
openssl dgst -sha512 -sigopt rsa_padding_mode:pss -sigopt rsa_pss_saltlen:64 -sign $tmp/rsa4096.key -out rsa4096-sig-pss-sha512.bin $tmp/msg

# TLS 1.3 CertificateVerify-style vectors: a 130-byte message signed by each leaf.
head -c 130 /dev/urandom > tls-msg.bin
openssl dgst -sha256 -sigopt rsa_padding_mode:pss -sigopt rsa_pss_saltlen:32 -sign $tmp/leafrsa.key -out tls-sig-rsa-pss-sha256.bin tls-msg.bin
openssl dgst -sha256 -sign $tmp/leaf.key -out tls-sig-p256-sha256.bin tls-msg.bin
rm -rf $tmp
echo "fixtures written; leaves valid from $(date -u +%Y-%m-%d) for 365 days"
