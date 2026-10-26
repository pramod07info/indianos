#!/usr/bin/env python3
"""Independent, OFFLINE Ed25519 vector check for actual 32/64KiB QEMU ELF.

Uses OpenSSL with publicly documented deterministic TEST seeds, not runtime
private keys. Never load these seeds into production OS trust stores.
"""
from __future__ import annotations
import hashlib
import struct
from pathlib import Path
from test_m2_phase4c_vectors import openssl_vectors,rust_constant,guest_elf

SOURCE=Path(__file__).resolve().parent.parent/"rust/kernel_core/src/m2_guest_qemu.rs"
DOMAIN=b"IndianOS/M2/LargeBundle/V1\0"

def large_elf(v:int)->bytes:
    length=32768 if v==1 else 65536
    data=bytearray(((i*31)&255)^0xA7^v for i in range(length))
    data[:147]=guest_elf(v)[:147]
    data[96:104]=struct.pack("<Q",length)
    data[104:112]=struct.pack("<Q",length)
    data[-1]=v
    return bytes(data)

def signed_large_digest(v:int)->tuple[bytes,bytes]:
    blob=large_elf(v)
    content=hashlib.sha256(blob).digest()
    canonical=(DOMAIN+struct.pack("<HIQII",1,9,v,len(blob),0)
         +content+struct.pack("<4I",0,0,0,0)
         +struct.pack("<4Q",0,0,0,0)+struct.pack("<IQ",7,2))
    assert len(canonical)==141
    return content,hashlib.sha256(canonical).digest()

def main()->None:
    rust=SOURCE.read_text(encoding="utf-8")
    publisher=rust_constant(rust,"TEST_PUBLISHER",32)
    for version in (1,2):
        content,message=signed_large_digest(version)
        assert rust_constant(rust,f"TEST_LARGE_DIGEST_{version}",32)==content
        public,sig=openssl_vectors(b"1"*32,message)
        assert public==publisher,"wrong Ed25519 trusted publisher root"
        assert rust_constant(rust,f"TEST_LARGE_SIG_{version}",64)==sig
        print(f"Large QEMU v{version}: {len(large_elf(version))} bytes, "
              f"real Ed25519 signature matches independent OpenSSL")
    print("IndianOS Phase 5C: independent large-ELF signatures VERIFIED")

if __name__=="__main__":
    main()
