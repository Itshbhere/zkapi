#!/usr/bin/env bash
set -euo pipefail
export HOMEBREW_NO_AUTO_UPDATE=1 HOMEBREW_NO_INSTALL_CLEANUP=1 HOMEBREW_NO_ANALYTICS=1

tap_dir=$(mktemp -d /tmp/zkapi-brew-tap.XXXXXX)
mkdir -p "$tap_dir/Formula"
sed -E 's@https://github.com/ethereum/zkapi/releases/download/clientd-v[^/]+/@file:///opt/artifacts/@g' \
  /opt/zkapi-package-test/zkapi-clientd.rb > "$tap_dir/Formula/zkapi-clientd.rb"
git init -q "$tap_dir"
git -C "$tap_dir" add Formula/zkapi-clientd.rb
git -C "$tap_dir" -c user.name='Package test' -c user.email='test@localhost' commit -qm 'Local release test'
formula=zkapi-validation/packages/zkapi-clientd
brew tap zkapi-validation/packages "$tap_dir"
brew install --formula "$formula"
brew test "$formula"
brew linkage --test "$formula"

# Preserve a clean profile and exercise Homebrew's own generated systemd unit.
config=/home/linuxbrew/zkapi-package-test-profile
mkdir -p /home/linuxbrew/.homebrew/services
printf 'ZKAPI_CLIENTD_CONFIG_DIR=%s\n' "$config" > /home/linuxbrew/.homebrew/services/zkapi-clientd.env
chmod 600 /home/linuxbrew/.homebrew/services/zkapi-clientd.env
brew services start "$formula"
service=$(systemctl --user list-unit-files --no-legend '*zkapi-clientd.service' | awk 'NR==1 {print $1}')
test -n "$service"
systemctl --user is-enabled --quiet "$service"
systemctl --user cat "$service"
systemd-analyze --user verify "/home/linuxbrew/.config/systemd/user/$service"
diagnosed=false
for _ in $(seq 1 30); do
  if grep -q 'Run zkapi-clientd config' /home/linuxbrew/.linuxbrew/var/log/zkapi-clientd.log; then
    diagnosed=true
    break
  fi
  sleep 1
done
test "$diagnosed" = true
test ! -f "$config/config.json"
# Homebrew 7.0.7 treats a unit waiting to restart after a configuration error
# as "not started". Stop that real unit with systemd before unregistering it.
systemctl --user stop "$service"
brew services stop "$formula"
state=$(systemctl --user show "$service" --property ActiveState --value)
test "$state" = inactive
if systemctl --user is-enabled --quiet "$service" 2>/dev/null; then
  echo 'Homebrew service remained enabled after stop' >&2
  exit 1
fi
brew uninstall --formula "$formula"
brew untap zkapi-validation/packages
echo 'PASS: Homebrew install, payload/formula test, linkage, generated user service, missing-profile guidance, systemd stop, and Homebrew unregister'
echo 'Not exercised: configured/funded inference, macOS binaries, or launchd runtime.'
