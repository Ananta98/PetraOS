export PS1='petraos:\w# '
export PATH=/usr/local/bin:/usr/bin:/bin:/usr/local/sbin:/usr/sbin:/sbin
export SHELL=/bin/bash
export TERM=linux
export HOME=/root
export XDG_CONFIG_HOME=/root/.config
export CHARSET=UTF-8
export LANG=C.UTF-8
export LC_ALL=C.UTF-8
alias ll='ls -la --color=auto'
alias l='ls -lh'

# Auto-start Hyprland + Quickshell UI on boot when DRM display is present
if [ -z "${WAYLAND_DISPLAY:-}" ] && [ -e /dev/dri/card0 ] && [ -x /usr/bin/start-petra-desktop ]; then
    /usr/bin/start-petra-desktop
fi
