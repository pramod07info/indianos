#!/usr/bin/env python3
"""M2 Phase 4C: independently reproduce QEMU-only Ed25519 fixtures.

Uses OpenSSL's Ed25519 implementation (not IndianOS's Rust implementation)
to cross-check the Rust QEMU manifest signature vectors and boot-policy
signature. Deterministic test seeds are PUBLIC, never production signing keys.
No external network, dependencies, writes or real secret material required.
All temporary files are removed before returning.
"""
from __future__ import annotations
import hashlib
from pathlib import Path
import re
import struct
import subprocess
import tempfile

SOURCE=Path(__file__).resolve().parent.parent/"rust/kernel_core/src/m2_guest_qemu.rs"
DOMAIN_MANIFEST=b"IndianOS/M2/SignedManifest/V1\0"
DOMAIN_ROOTS=b"IndianOS/M2/PublisherRoots/V1\0"

def rust_constant(source:str,name:str,size:int)->bytes:
    pattern=rf"const\s+{re.escape(name)}:\[u8;{size}\]=\[([^\]]+)\];"
    found=re.search(pattern,source)
    if not found:
        raise AssertionError(f"Missing {name} Rust vector")
    data=bytes(int(n.strip()) for n in found.group(1).split(",") if n.strip())
    if len(data)!=size:
        raise AssertionError(f"Unexpected {name} bytes: {len(data)}")
    return data

def openssl_vectors(seed:bytes,message:bytes)->tuple[bytes,bytes]:
    assert len(seed)==32
    with tempfile.TemporaryDirectory(prefix="indianos-phase4c-test-only-") as tmp:
        root=Path(tmp)
        key=root/"test-only-key.der"
        key.write_bytes(bytes.fromhex("302e020100300506032b657004220420")+seed)
        raw=root/"data.bin";raw.write_bytes(message)
        pub=root/"public.der"
        sig=root/"signature.bin"
        def run(args:list[str])->None:
            subprocess.run(args,check=True,stdin=subprocess.DEVNULL,
                           stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        run(["openssl","pkey","-inform","DER","-in",str(key),
             "-pubout","-outform","DER","-out",str(pub)])
        run(["openssl","pkeyutl","-sign","-rawin","-keyform","DER",
             "-inkey",str(key),"-in",str(raw),"-out",str(sig)])
        public=pub.read_bytes()
        assert public.startswith(bytes.fromhex("302a300506032b6570032100"))
        return public[-32:],sig.read_bytes()

def guest_elf(version:int)->bytes:
    image=bytearray(2400)
    image[:4]=b"\x7fELF"
    image[4:7]=bytes([2,1,1])
    image[16:18]=struct.pack("<H",2)
    image[18:20]=struct.pack("<H",62)
    image[20:24]=struct.pack("<I",1)
    image[24:32]=struct.pack("<Q",0x400078)
    image[32:40]=struct.pack("<Q",64)
    image[52:54]=struct.pack("<H",64)
    image[54:56]=struct.pack("<H",56)
    image[56:58]=struct.pack("<H",1)
    image[64:68]=struct.pack("<I",1)
    image[68:72]=struct.pack("<I",5)
    image[80:88]=struct.pack("<Q",0x400000)
    image[96:104]=struct.pack("<Q",2400)
    image[104:112]=struct.pack("<Q",2400)
    image[112:120]=struct.pack("<Q",4096)
    image[120:147]=bytes([
      0xB8,7,0,0,0,0x31,0xFF,0x31,0xF6,0xCD,0x80,
      0x48,0x83,0xF8,0xFF,0x75,0x08,
      0xB8,2,0,0,0,0xCD,0x80,0xF4,0x0F,0x0B])
    image[-1]=version
    return bytes(image)

def manifest_digest(version:int)->bytes:
    file_digest=hashlib.sha256(guest_elf(version)).digest()
    canonical=(DOMAIN_MANIFEST+struct.pack("<HIQIQI",1,7,2,9,version,0)
        +file_digest+struct.pack("<4I",0,0,0,0)
        +struct.pack("<4Q",0,0,0,0))
    assert len(canonical)==140
    return hashlib.sha256(canonical).digest()

def main()->None:
    rust=SOURCE.read_text(encoding="utf-8")
    publisher=rust_constant(rust,"TEST_PUBLISHER",32)
    boot_root=rust_constant(rust,"TEST_BOOT_ROOT",32)
    actual_pub,_=openssl_vectors(b"1"*32,b"cross-check")
    assert actual_pub==publisher,"QEMU publisher public key does not match"
    for version in (1,2):
        public,signature=openssl_vectors(b"1"*32,manifest_digest(version))
        assert public==publisher
        actual=rust_constant(rust,f"TEST_APP_SIG_{version}",64)
        assert signature==actual,f"Invalid M2P6 application v{version} Ed25519 signature"
    anchor=(struct.pack("<IQ",7,2)+publisher+bytes([1]))
    policy=(DOMAIN_ROOTS+struct.pack("<QI",3,1)
        +anchor+bytes(45*3))
    assert len(policy)==222
    boot_public,boot_sig=openssl_vectors(b"R"*32,hashlib.sha256(policy).digest())
    assert boot_public==boot_root
    assert boot_sig==rust_constant(rust,"TEST_BOOT_SIG",64)
    print("M2 Phase 4C OK: real OpenSSL Ed25519 independently confirms")
    print("  2 M2P6 signed native ELF versions and 1 signed boot policy")
    print("  Public, deterministic test vectors only; NO production keys.")

if __name__=="__main__":
    main()
