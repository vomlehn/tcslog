# Puts the tcslog package's programs and pkg-config file where a login
# shell finds them. Each directory is added only if it is not there
# already, so this file and the other TCS packages' can all be sourced,
# in any order and any number of times, without the paths growing.

case ":${PATH}:" in
*:/opt/tcs/bin:*)
	;;
*)
	PATH="${PATH:+${PATH}:}/opt/tcs/bin"
	;;
esac
export PATH

case ":${PKG_CONFIG_PATH:-}:" in
*:/opt/tcs/lib/pkgconfig:*)
	;;
*)
	PKG_CONFIG_PATH="${PKG_CONFIG_PATH:+${PKG_CONFIG_PATH}:}"
	PKG_CONFIG_PATH="${PKG_CONFIG_PATH}/opt/tcs/lib/pkgconfig"
	;;
esac
export PKG_CONFIG_PATH
