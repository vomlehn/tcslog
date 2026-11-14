# shellcheck shell=sh
# Puts the tcslog-gen package's program where a login shell finds it. The
# directory is added only if it is not there already, so this file and the
# other TCS packages' can all be sourced, in any order and any number of
# times, without the path growing.

case ":${PATH}:" in
*:/opt/tcs/bin:*)
	;;
*)
	PATH="${PATH:+${PATH}:}/opt/tcs/bin"
	;;
esac
export PATH
