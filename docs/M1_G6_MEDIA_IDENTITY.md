# G6 filesystem media identity

Direct `DurableFs` and `HierVfs` mounts now retain the block driver's optional
media generation as well as sector count. The existing `BlockWindow` identity
preflight is reused; this adds no new filesystem, locking table or disk format.

Previously a same-capacity reset or replacement with identical metadata heads
could pass a direct mount's geometry and checkpoint checks. Selected partition
windows already rejected this case, but direct filesystem mounts did not.
Mounting, read-only health inspection and explicit blank-device formatting now
check a stable generation through admission. Live operations check the pinned
identity wherever geometry was already validated. Once a live mount observes
an identity change, it is retired even if a later token matches the old one.
Token appearance or disappearance is also a change. An explicit fresh mount
must revalidate the medium before reads or mutations resume.

Four regression tests cover unchanged-capacity reset, token loss/appearance,
unchanged outputs and no I/O after revocation, changes during admission or
format preflight, payload-read changes, and data-write/flush changes. They also
exercise fresh-mount recovery without overwriting the prior checkpoint.
The tests were authored for GitHub CI; no local builds, tests or QEMU ran.

Drivers returning `None` cannot detect an identical same-capacity replacement;
they retain the existing checksum/head validation contract. The driver must
increment its supplied token after reset or media replacement. The owner must
still serialize complete filesystem operations; these checks are not SMP
locking or proof of physical power-loss durability. G6 acceptance still needs
the exact integrated CI/QEMU evidence and applicable hardware qualification.
