#!/usr/bin/env python3
"""Checks the last signature of a PDF with OpenSSL, for tests.

    tests/signing/verify.py FILE.pdf [CA.pem]

The signature's /ByteRange must cover the whole file except its /Contents, and
`openssl cms -verify` must accept the CMS signature in /Contents over those bytes, with the
certificate chain ending in CA.pem (default: the test CA next to this script). Prints `ok`, or
what is wrong and exits with 1.
"""

import os
import re
import subprocess
import sys
import tempfile


def main() -> int:
    pdf_path = sys.argv[1]
    ca = sys.argv[2] if len(sys.argv) > 2 else os.path.join(os.path.dirname(__file__), "ca.pem")
    with open(pdf_path, "rb") as file:
        pdf = file.read()
    ranges = list(re.finditer(rb"/ByteRange\s*\[\s*(\d+)\s+(\d+)\s+(\d+)\s+(\d+)\s*\]", pdf))
    if not ranges:
        print(f"{pdf_path}: no signature")
        return 1
    a, b, c, d = (int(value) for value in ranges[-1].groups())
    if a != 0 or c + d != len(pdf):
        print(f"{pdf_path}: the byte range [{a} {b} {c} {d}] does not cover the file ({len(pdf)} bytes)")
        return 1
    contents = pdf[b:c]
    if not (contents.startswith(b"<") and contents.endswith(b">")):
        print(f"{pdf_path}: the gap of the byte range is not the hexadecimal string")
        return 1
    if not re.search(rb"/Contents\s*$", pdf[:b]):
        print(f"{pdf_path}: the gap of the byte range is not /Contents")
        return 1
    signature = bytes.fromhex(contents[1:-1].decode("ascii"))
    # DER ends where its outer length says; the rest is padding.
    length = signature[1]
    if length & 0x80:
        count = length & 0x7F
        length = 2 + count + int.from_bytes(signature[2 : 2 + count], "big")
    else:
        length += 2
    signature = signature[:length]
    with tempfile.TemporaryDirectory() as directory:
        der = os.path.join(directory, "signature.der")
        data = os.path.join(directory, "data")
        with open(der, "wb") as file:
            file.write(signature)
        with open(data, "wb") as file:
            file.write(pdf[a:b] + pdf[c : c + d])
        result = subprocess.run(
            ["openssl", "cms", "-verify", "-binary", "-inform", "DER", "-in", der, "-content", data,
             "-CAfile", ca, "-purpose", "any", "-out", os.devnull],
            capture_output=True,
            text=True,
            check=False,
        )
    if result.returncode != 0:
        print(f"{pdf_path}: openssl cms -verify failed: {result.stderr.strip()}")
        return 1
    print("ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
