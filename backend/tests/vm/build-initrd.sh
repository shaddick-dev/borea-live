#!/bin/sh
# Only run inside the test-tools container, with a marked disposable /work bind mount.
set -eu
test "$(cat /work/.borea-test-workdir)" = BOREA_TEST_WORK
mkdir -p /work/rootfs
cd /
tar --one-file-system --exclude=./proc --exclude=./sys --exclude=./dev --exclude=./run --exclude=./work --exclude=./binaries --exclude=./tmp --exclude=./boot -cf - . | tar -xf - -C /work/rootfs
mkdir -p /work/rootfs/dev /work/rootfs/proc /work/rootfs/sys /work/rootfs/run /work/rootfs/tmp /work/rootfs/boot
test -e /work/rootfs/dev/console || mknod -m 0600 /work/rootfs/dev/console c 5 1
test -e /work/rootfs/dev/null || mknod -m 0666 /work/rootfs/dev/null c 1 3
cp /work/test-init /work/rootfs/init
cp /work/borea-dhcp /work/rootfs/borea-dhcp
chmod 0755 /work/rootfs/init /work/rootfs/borea-dhcp
for binary in borea-vm-test borea-firstboot; do cp "/binaries/$binary" "/work/rootfs/usr/bin/$binary"; done
mkdir -p /work/rootfs/usr/share/keyrings/borea
cp /work/keyrings/*.gpg /work/rootfs/usr/share/keyrings/borea/
cp /boot/vmlinuz-* /work/vmlinuz
cd /work/rootfs
find . -print0 | cpio --null -o --format=newc 2>/work/cpio.log | gzip -1 > /work/live.cpio.gz.new
mv /work/live.cpio.gz.new /work/live.cpio.gz
