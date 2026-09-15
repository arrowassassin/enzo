#!/usr/bin/env python3
"""Builds roots.bin, the trust store quire-net's TLS verifier uses, from roots.pem.

Each root keeps only what path building needs: its subject name and its
SubjectPublicKeyInfo, both as complete DER elements. Record layout, repeated:

    u8  name length, name (UTF-8, the CN, for logs)
    u16 subject length (little-endian), subject DER
    u16 spki length (little-endian), spki DER

Run from this folder after editing roots.pem: `python3 build.py`.
"""
import base64, struct, sys, pathlib

def tlv(b, i):
    """Return (tag, header length, content length) of the element at i."""
    tag = b[i]; l = b[i + 1]; h = 2
    if l & 0x80:
        n = l & 0x7f; l = int.from_bytes(b[i + 2:i + 2 + n], 'big'); h = 2 + n
    return tag, h, l

def children(b, i):
    """Offsets of the elements inside the constructed element at i."""
    _, h, l = tlv(b, i)
    j, end, out = i + h, i + h + l, []
    while j < end:
        _, hh, ll = tlv(b, j); out.append(j); j += hh + ll
    return out

def whole(b, i):
    _, h, l = tlv(b, i); return b[i:i + h + l]

def cn(b, name_off):
    """The CN attribute of a Name, for the record label."""
    for rdn in children(b, name_off):
        for atv in children(b, rdn):
            oid, val = children(b, atv)
            if whole(b, oid) == b'\x06\x03\x55\x04\x03':
                _, h, l = tlv(b, val); return b[val + h:val + h + l].decode('utf-8', 'replace')
    return '?'

def main():
    here = pathlib.Path(__file__).parent
    pem = (here / 'roots.pem').read_text()
    out = bytearray(); names = []
    for block in pem.split('-----BEGIN CERTIFICATE-----')[1:]:
        der = base64.b64decode(block.split('-----END CERTIFICATE-----')[0])
        tbs = children(der, 0)[0]
        fields = children(der, tbs)
        if der[fields[0]] == 0xA0:  # explicit version
            fields = fields[1:]
        # serial, signature, issuer, validity, subject, spki
        subject = whole(der, fields[4]); spki = whole(der, fields[5])
        name = cn(der, fields[4]).encode()
        out += struct.pack('<B', len(name)) + name
        out += struct.pack('<H', len(subject)) + subject
        out += struct.pack('<H', len(spki)) + spki
        names.append(name.decode())
    (here / 'roots.bin').write_bytes(out)
    print(f'{len(names)} roots, {len(out)} bytes:', ', '.join(names))

if __name__ == '__main__':
    main()
