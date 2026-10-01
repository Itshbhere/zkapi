#!/usr/bin/env bash
set -euo pipefail

# Internal entry point for test-docker.sh, in its disposable Arch container.
[[ $(id -u) == 0 && -d /package-source ]] || {
    echo 'Use packaging/arch/test-docker.sh on a Linux Docker host.' >&2
    exit 2
}
# The minimal Docker image excludes package documentation; restore it so the
# payload check tests a complete installation, as on a normal Arch machine.
sed -i '/^NoExtract.*usr\/share\/doc/d' /etc/pacman.conf
pacman -Syu --noconfirm --needed base-devel ca-certificates openssl gcc-libs python libarchive
useradd --create-home --uid 1000 package-test
install -d -o package-test -g package-test /home/package-test/build
cp /package-source/PKGBUILD /package-source/.SRCINFO /home/package-test/build/
if [[ -d /release-cache ]]; then
    case $(uname -m) in
        x86_64) target=amd64 ;;
        aarch64) target=arm64 ;;
        *) echo 'The AUR package supports x86_64 and aarch64.' >&2; exit 1 ;;
    esac
    # Only seed a matching cached download; its pinned SHA-256 is checked below.
    version=$(bash -c 'source /package-source/PKGBUILD; printf "%s" "$pkgver"')
    archive="/release-cache/zkapi-clientd_${version}_linux_${target}.tar.gz"
    [[ ! -f "$archive" ]] || cp "$archive" /home/package-test/build/
fi
chown -R package-test:package-test /home/package-test/build

runuser -u package-test -- bash <<'BUILD'
set -euo pipefail
cd /home/package-test/build
makepkg --printsrcinfo > generated.SRCINFO
python - <<'PY'
from collections import defaultdict
from pathlib import Path

def metadata(path):
    fields = defaultdict(list)
    for line in Path(path).read_text().splitlines():
        if line.strip() and not line.lstrip().startswith("#"):
            key, value = line.strip().split(" = ", 1)
            fields[key].append(value)
    return dict(fields)

assert metadata(".SRCINFO") == metadata("generated.SRCINFO"), "Stale .SRCINFO"
PY
makepkg --noconfirm
BUILD

mapfile -t packages < <(runuser -u package-test -- bash -c \
    'cd /home/package-test/build && makepkg --packagelist')
[[ ${#packages[@]} == 1 ]]
pacman -U --noconfirm "${packages[0]}"
pacman -Qkk zkapi-clientd-bin
version=$(bash -c 'source /package-source/PKGBUILD; printf "%s" "$pkgver"')
[[ $(zkapi-clientd version) == "zkapi-clientd $version" ]]
zkapi-walletd --help >/dev/null
if ldd /usr/bin/zkapi-walletd | grep -q 'not found'; then
    echo 'The companion has missing dynamic libraries.' >&2
    exit 1
fi

# Every public asset, including the proof setup, must survive makepkg/pacman
# without mutation. The companion discovers share relative to its binary.
python - <<'PY'
from pathlib import Path

source = Path("/home/package-test/build/src")
for name in ("zkapi-clientd", "zkapi-walletd"):
    assert (source / name).read_bytes() == (Path("/usr/bin") / name).read_bytes(), name
for original in (source / "share/zkapi-clientd").rglob("*"):
    if original.is_file():
        installed = Path("/usr") / original.relative_to(source)
        assert installed.read_bytes() == original.read_bytes(), str(installed)
for name in ("request.pk", "request.vk", "withdrawal.pk", "withdrawal.vk", "manifest.json"):
    proof = Path("/usr/bin/zkapi-walletd").resolve().parent.parent / "share/zkapi-clientd/proof-setup" / name
    assert proof.is_file() and proof.stat().st_size > 0, str(proof)
assert (source / "zkapi-clientd.service").read_bytes() == Path("/usr/lib/systemd/user/zkapi-clientd.service").read_bytes()
PY

loginctl enable-linger package-test
systemctl start user@1000.service
runuser -u package-test -- env XDG_RUNTIME_DIR=/run/user/1000 \
    systemd-analyze --user verify /usr/lib/systemd/user/zkapi-clientd.service
user_systemctl() {
    runuser -u package-test -- env XDG_RUNTIME_DIR=/run/user/1000 \
        DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus systemctl --user "$@"
}
user_systemctl daemon-reload
user_systemctl enable zkapi-clientd.service
user_systemctl is-enabled --quiet zkapi-clientd.service
user_systemctl start zkapi-clientd.service

# No funding or configuration is performed by installation or the service. A
# fresh profile must execute the installed daemon and explain what is missing.
guidance=false
for (( attempt=0; attempt<30; attempt++ )); do
    journalctl --no-pager _SYSTEMD_USER_UNIT=zkapi-clientd.service > /tmp/clientd-service.log
    # The diagnostic is written before exit; wait for systemd to reap it too.
    if grep -q 'Run zkapi-clientd config' /tmp/clientd-service.log &&
        [[ $(user_systemctl show zkapi-clientd.service --property=ExecMainStatus --value) == 1 ]]; then
        guidance=true
        break
    fi
    sleep 1
done
"$guidance" || { cat /tmp/clientd-service.log >&2; exit 1; }
[[ $(user_systemctl show zkapi-clientd.service --property=Restart --value) == on-failure ]]
user_systemctl stop zkapi-clientd.service
[[ $(user_systemctl show zkapi-clientd.service --property=ActiveState --value) == inactive ]]
user_systemctl disable zkapi-clientd.service
[[ ! -e /home/package-test/.config/zkapi-clientd/config.json ]]
[[ ! -e /home/package-test/.config/zkapi-clientd/zkapi ]]

cat /tmp/clientd-service.log
printf 'Arch package %s: makepkg, pacman installation, binary/proof integrity, and real systemd user-service enable/start/configuration guidance/stop passed.\n' "$version"
