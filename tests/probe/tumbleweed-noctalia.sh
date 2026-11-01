#!/bin/bash
# What Tumbleweed offers for Noctalia v5 with Hyprland, niri and MangoWC:
# names, versions, repositories, and the files Borea's recipe relies on.
set -u
zypper -n --gpg-auto-import-keys refresh >/dev/null
echo "== Official packages"
for p in noctalia noctalia-greeter greetd hyprland xdg-desktop-portal-hyprland niri \
         xdg-desktop-portal-gnome xwayland-satellite playerctl xdg-desktop-portal-wlr \
         accountsservice xdg-desktop-portal-gtk xdg-user-dirs pipewire pipewire-pulseaudio \
         wireplumber polkit dbus-1 dejavu-fonts foot plymouth plymouth-dracut flatpak \
         mangowc mangowm mango; do
  info=$(zypper -n --no-refresh info "$p" 2>/dev/null | grep -E '^(Name|Version|Repository) *:' | awk -F': ' '{print $2}' | paste -sd' ')
  echo "$p: ${info:-NOT FOUND}"
done
echo "== Community OBS projects building MangoWC for Tumbleweed"
for proj in X11:Wayland home:osranze:oS home:darix:playground home:team-vrock home:neifua:Noctalia; do
  path=$(echo "$proj" | sed 's/:/:\//g')
  url="https://download.opensuse.org/repositories/$path/openSUSE_Tumbleweed/"
  if zypper -n --gpg-auto-import-keys addrepo --refresh "$url" "probe-$proj" >/dev/null 2>&1 \
     && zypper -n --gpg-auto-import-keys refresh "probe-$proj" >/dev/null 2>&1; then
    found=$(zypper -n --no-refresh search -s -r "probe-$proj" mango 2>/dev/null | grep -E '^\S* *\|' | grep -i mango | tr -s ' ')
    key=$(rpm -qa 'gpg-pubkey*' --qf '%{SUMMARY} %{VERSION}-%{RELEASE}\n' | grep -i "$(echo "$proj" | cut -d: -f2)" | head -1)
    echo "$proj: ${found:-no mango package} | key: ${key:-?} | $url"
  else
    echo "$proj: no Tumbleweed repository"
  fi
done
echo "== Files the recipe relies on"
zypper -n --no-refresh install --no-recommends noctalia noctalia-greeter greetd hyprland niri >/tmp/install.log 2>&1 \
  || tail -20 /tmp/install.log
for f in /usr/bin/noctalia /usr/bin/noctalia-greeter-session /usr/bin/noctalia-greeter-apply-appearance \
         /usr/bin/start-hyprland /usr/bin/Hyprland /usr/bin/niri-session /usr/bin/niri /usr/bin/greetd; do
  [ -e "$f" ] && echo "present $f" || echo "MISSING $f"
done
rpm -ql noctalia-greeter | grep -E 'tmpfiles|sysusers|bin/|greetd' | head -20
echo "greetd user: $(grep -h greeter /usr/lib/sysusers.d/*.conf 2>/dev/null | head -3)"
echo "greetd config: $(rpm -qc greetd | head -3)"
hyprctl version 2>/dev/null | head -2 || rpm -q hyprland
niri --version 2>/dev/null || rpm -q niri
noctalia --version 2>/dev/null || rpm -q noctalia
