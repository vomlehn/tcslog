# shellcheck shell=sh
# Puts the tcslog package's pkg-config file where a login shell finds it.
# The directory is added only if it is not there already, so this file and
# the other TCS packages' can all be sourced, in any order and any number
# of times, without the path growing.

case ":${PKG_CONFIG_PATH:-}:" in
*:/opt/tcs/lib/pkgconfig:*)
	;;
*)
	PKG_CONFIG_PATH="${PKG_CONFIG_PATH:+${PKG_CONFIG_PATH}:}"
	PKG_CONFIG_PATH="${PKG_CONFIG_PATH}/opt/tcs/lib/pkgconfig"
	;;
esac
export PKG_CONFIG_PATH
