# AI-native app discovery and installation

**Status:** AI app assistant, system app store and on-device installer are still designs. R030 separately provides an offline developer-host Ed25519 verifier, native Rust SDK and transactional staging CLI tested with QEMU; it is not an IndianOS kernel package service.

## Product promise

An IndianOS user should eventually be able to say: "Install an editor for Java, preferably free and offline." The assistant will search trusted catalogs, explain compatibility, costs, privacy and permissions, then present an *install plan*. It will not silently change privileged settings.

## Non-negotiable trust boundary

The language model is **untrusted for execution**. It may suggest intent and explain choices but must not directly execute shell scripts, run privileged commands, install unknown binaries, bypass signatures, modify security policy or decide licensing compliance on its own.

## Workflow

1. User prompt, e.g. "Install a PDF reader".
2. Intent parser returns a strictly validated request: app category, preferred language, price, offline need, permissions.
3. Signed metadata catalog returns candidate applications with provenance and versions.
4. Deterministic resolver selects execution routes based on verified capability inventory, OS/CPU architecture, device resources, risk and license.
5. Security service checks approved repositories, artifact hashes, signatures, dependency graph, installation scope, sandbox permissions, privacy and available disk space.
6. Assistant presents candidate routes, reasons, expected resource use and limitations; **user explicitly approves** selected action.
7. Privileged package service downloads from allowlisted sources using authenticated transport, re-verifies signatures, installs transactionally and writes an audit log.
8. Launch/smoke-test, update state and rollback if validation fails. Report errors honestly.

## Execution privilege model

    Unprivileged UI / optional language model
                     |
          Typed app-plan request
                     |
        Offline deterministic planner
                     |
       Explicit user approval/consent
                     |
       Signed policy-gated package daemon
                     |
         Sandbox and transactional install
                     |
              Audited result

No natural language is passed through a privileged shell. Avoid accepting an AI-generated URL as a trust source. Never promote a compatibility route from "planned" to "verified" without real tests.

## Security

- Least privilege by default; per-app filesystem, camera, network and microphone permissions.
- Offline-first policy and optional local model; cloud AI only with opt-in and clear data handling.
- Defend prompt-injection in catalog descriptions, package metadata and web pages.
- Reproducible builds and dependency/SBOM audit for distributable apps.
- Signed catalogs and packages, pinned dependencies, anti-downgrade and rollback.
- Explicit confirmation for app installation, remote Mac access, VM image creation or paid services.
- Separate credentials and protected secrets; no assistant access to kernel debug or firmware privileges.
- Safe fallback: unknown app -> unsupported, no guessing a package source.

## Host-side reference planner

The repository includes tools/app_resolution.py, examples/app_catalog.json, examples/device_profile.json and tests/test_app_resolution.py. This is an **offline development reference only**, not an AI model, app downloader, installer, package manager or proof that the kernel supports external apps.

From repository root on a computer with Python 3.10+:

    python3 tools/app_resolution.py --catalog examples/app_catalog.json --device examples/device_profile.json --app "Sample Notes"
    python3 -m unittest discover -s tests -v

These commands never contact the network or install applications.

## Future milestones

- Catalog schema, manifest signing and signed software source governance
- Native package manager transactions and rollback
- Deterministic route planner integrated with real device inventory
- Language model with constrained structured-output intent parsing and source citations
- Hindi / English voice and text UI; additional Indian languages later
- Compatibility provider API and tests
- Opt-in local-first assistant with privacy controls
