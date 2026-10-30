# Public release verification keys

These files contain public keys only. No private signing material is included.

- Arch: the previously reviewed project release key, primary fingerprint
  `3E80CA1A8B89F69CBA57D98A76A5EF9054449A5C`, copied from the existing Arch adapter.
- Fedora: release 43–46 public primary keys from the official `fedora:44` image,
  registry manifest digest `sha256:43b29f65a41eb9c35e1cd5323e3bdf3b655c2357a9f4f1ff2f9c2798e5045d80`.
- openSUSE: the public RPM verification keys bundled in the official Tumbleweed
  image, registry manifest digest `sha256:2e73c9a73aed4edfdd8a383608368a7ee6b8de2a2aa262843c89bb6778c2c24d`.
- Omarchy: the ISO signing key `Omarchy <pkgs@omarchy.org>`, primary fingerprint
  `40DFB630FF42BCFFB047046CF0134EE680CAC571`, from `builder/omarchy.gpg` in
  https://github.com/omacom-io/omarchy-iso at `4df07ef`. The fingerprint matches the
  one that repository's README tells users to verify release ISOs with. Borea's
  hand-off (`crates/borea-handoff`) requires a valid signature by it on the
  pinned Omarchy ISO. `omarchy-iso-key.asc` is the same key, armored, for review.

The initial Fedora/openSUSE key provenance is the official registry publication
with the above pinned digest. The `.gpg` files are dearmored public certificate
bundles; `fingerprints.txt` records their keys and identities for review. Release
metadata and package availability still require live verification against the
upstream mirrors; this bundle does not certify any install pathway. The cloud
proxy currently refuses the Arch/openSUSE download hosts with HTTP 403.
