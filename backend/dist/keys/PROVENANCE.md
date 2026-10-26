# Public release verification keys

These files contain public keys only. No private signing material is included.

- Arch: the previously reviewed project release key, primary fingerprint
  `3E80CA1A8B89F69CBA57D98A76A5EF9054449A5C`, copied from the existing Arch adapter.
- Fedora: release 43–46 public primary keys from the official `fedora:44` image,
  registry manifest digest `sha256:43b29f65a41eb9c35e1cd5323e3bdf3b655c2357a9f4f1ff2f9c2798e5045d80`.
- openSUSE: the public RPM verification keys bundled in the official Tumbleweed
  image, registry manifest digest `sha256:2e73c9a73aed4edfdd8a383608368a7ee6b8de2a2aa262843c89bb6778c2c24d`.

The initial Fedora/openSUSE key provenance is the official registry publication
with the above pinned digest. The `.gpg` files are dearmored public certificate
bundles; `fingerprints.txt` records their keys and identities for review. Release
metadata and package availability still require live verification against the
upstream mirrors; this bundle does not certify any install pathway. The cloud
proxy currently refuses the Arch/openSUSE download hosts with HTTP 403.
