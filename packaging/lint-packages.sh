#!/bin/sh
# Runs the packaging checkers on what `make packages` built.
#
#   packaging/lint-packages.sh <rpmlint-config> <package>... -- <script>...
#
# <rpmlint-config> is packaging/rpm/rpmlint.toml, which says what rpmlint
# is to accept and why; each <package> a .deb or an .rpm; each <script> a
# shell script, as installed where it has placeholders filled in. lintian
# reads the overrides each .deb carries, so it is given nothing more.
# `make lint-packages` runs it.
#
# A checker that is not installed is skipped, in one line, so that a
# machine without rpmlint can still check the .debs. Any finding from a
# checker that ran is a failure: what is intended is overridden, with its
# reason, and anything else is to be fixed.
set -eu

if [ $# -lt 1 ]; then
	echo "usage: lint-packages.sh <rpmlint-config> <package>..." \
	     "-- <script>..." >&2
	exit 2
fi
config=$1
shift

debs=
rpms=
while [ $# -gt 0 ] && [ "$1" != -- ]; do
	case $1 in
	*.deb)
		debs="$debs $1"
		;;
	*.rpm)
		rpms="$rpms $1"
		;;
	*)
		echo "lint-packages.sh: $1 is neither a .deb nor an .rpm" >&2
		exit 2
		;;
	esac
	shift
done
if [ $# -gt 0 ]; then
	shift
fi

if [ -z "$debs" ] && [ -z "$rpms" ]; then
	echo "lint-packages.sh: there are no packages to check: run" \
	     "make packages first" >&2
	exit 1
fi
if [ $# -eq 0 ]; then
	echo "lint-packages.sh: there are no staged scripts to check: run" \
	     "make packages first" >&2
	exit 1
fi

status=0

if ! command -v lintian > /dev/null 2>&1; then
	echo "lintian is not installed, so the .debs were not checked"
elif [ -z "$debs" ]; then
	echo "There are no .debs, so lintian checked nothing"
else
	echo "Checking the .debs with lintian..."
	# Warnings fail as errors do. Each package's overrides are its own,
	# in /usr/share/lintian/overrides, and lintian reports one that
	# matches nothing, which is a stale override to take away.
	# shellcheck disable=SC2086 # One package per word.
	lintian --fail-on error,warning --tag-display-limit 0 $debs \
		|| status=1
fi

if ! command -v rpmlint > /dev/null 2>&1; then
	echo "rpmlint is not installed, so the RPMs were not checked"
elif [ -z "$rpms" ]; then
	echo "There are no RPMs, so rpmlint checked nothing"
else
	echo "Checking the RPMs with rpmlint..."
	# --strict makes a warning an error, so that either fails.
	# shellcheck disable=SC2086 # One package per word.
	rpmlint --strict -c "$config" $rpms || status=1
fi

if ! command -v shellcheck > /dev/null 2>&1; then
	echo "shellcheck is not installed, so the scripts were not checked"
else
	echo "Checking the shell scripts with shellcheck..."
	shellcheck "$@" || status=1
fi

if [ "$status" = 0 ]; then
	echo "[OK] No packaging checker found anything"
fi
exit $status
