#!/bin/sh
# Adds to a .deb's tree what Debian expects of every package and the stage
# does not hold.
#
#   packaging/deb/debian-files.sh <source> <package> <root> <version> <epoch>
#
# <source> is this repository's name, the source package's; <package> the
# package being built; <root> the tree dpkg-deb is about to build it from;
# <version> the Debian version, upstream and revision; and <epoch> the
# release's time in seconds since 1970, which dates the changelog entry.
# `make deb` runs it once per package, after the stage has been copied to
# <root>.
#
# These are the standard files outside /opt/tcs, as the hooks are, because
# Debian's tools look for them only where Debian keeps them:
#
#   /usr/share/doc/<package>/copyright, in Debian's machine-readable format,
#   from packaging/deb/copyright.in with the copyright line and the MIT
#   licence's text taken from LICENSE-MIT, so that the text is stated once;
#   /usr/share/doc/<package>/changelog.Debian.gz, one entry for <version>,
#   so that a new version needs no edit here;
#   /usr/share/lintian/overrides/<package>, from
#   packaging/deb/<package>/lintian-overrides if there is one, which says
#   why each finding it overrides is intended; and
#   DEBIAN/triggers, for a package with a shared library in /opt/tcs/lib,
#   which has dpkg run ldconfig once at the end of the run rather than each
#   maintainer script running it, as debhelper arranges for a library.
#
# The .deb alone has them: the RPM says the same things its own way, in its
# License and %changelog, %post and %postun, and in packaging/rpm's rpmlint
# configuration, which is not shipped.
set -eu

source=$1
package=$2
root=$3
version=$4
epoch=$5

here=$(dirname "$0")
licence=$here/../../LICENSE-MIT
doc=$root/usr/share/doc/$package

# The years and holder, as LICENSE-MIT's own copyright line gives them.
copyright=$(sed -n 's/^Copyright (c) //p' "$licence")
if [ -z "$copyright" ]; then
	echo "debian-files.sh: $licence has no 'Copyright (c)' line to take" \
	     "the copyright from" >&2
	exit 1
fi

# Each directory made here is made 0755, whatever the umask, as the stage's
# are.
install -d -m 755 "$root/usr" "$root/usr/share" "$root/usr/share/doc" "$doc"

# The licence's text is everything after its copyright line, less the
# blank lines before it, written as a field's continuation lines are: each
# indented by a space, an empty one as a lone ".".
awk -v copyright="$copyright" -v package="$package" -v licence="$licence" '
/^@MIT@$/ {
	body = 0
	while ((getline line < licence) > 0) {
		if (line ~ /^Copyright \(c\) /) {
			body = 1
			started = 0
			continue
		}
		if (!body || (!started && line == ""))
			continue
		started = 1
		print (line == "" ? " ." : " " line)
	}
	next
}
{
	gsub(/@COPYRIGHT@/, copyright)
	gsub(/@PACKAGE@/, package)
	print
}' "$here/copyright.in" > "$doc/copyright"
chmod 644 "$doc/copyright"

# The upstream changes are in the package's CHANGELOG.md where it has one,
# so the entry points there rather than repeating them.
changes="$source ${version%-*}, as released upstream."
if [ -f "$root/opt/tcs/share/doc/$package/CHANGELOG.md" ]; then
	changes="$changes See /opt/tcs/share/doc/$package/CHANGELOG.md."
fi
{
	echo "$source ($version) unstable; urgency=medium"
	echo
	echo "$changes" | fold -s -w 72 | sed -e 's/ *$//' -e '1s/^/  * /' \
		-e '2,$s/^/    /'
	echo
	echo " -- David VomLehn <dvomlehn@gmail.com>  $(date -u -R -d "@$epoch")"
} | gzip -9n > "$doc/changelog.Debian.gz"
chmod 644 "$doc/changelog.Debian.gz"

overrides=$here/$package/lintian-overrides
if [ -f "$overrides" ]; then
	install -d -m 755 "$root/usr/share/lintian" \
		"$root/usr/share/lintian/overrides"
	install -m 644 "$overrides" \
		"$root/usr/share/lintian/overrides/$package"
fi

for library in "$root"/opt/tcs/lib/lib*.so.[0-9]*; do
	if [ -f "$library" ]; then
		echo "activate-noawait ldconfig" > "$root/DEBIAN/triggers"
		chmod 644 "$root/DEBIAN/triggers"
		break
	fi
done
