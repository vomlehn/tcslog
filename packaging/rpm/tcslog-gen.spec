# The tcslog-gen RPM, built by `make rpm` from the tree `make stage`
# installed under build/stage/tcslog-gen. Nothing is compiled here: %install
# copies that tree, so the RPM and the .deb hold the same files, built
# once. The Makefile passes tcs_version, from Cargo.toml; tcs_release,
# the Debian revision, so that the two are named alike; tcs_stage;
# and tcs_date, the date of the commit built, for the %changelog.

%{!?tcs_version: %{error: tcs_version is not defined: build with make rpm}}
%{!?tcs_release: %{error: tcs_release is not defined: build with make rpm}}
%{!?tcs_stage: %{error: tcs_stage is not defined: build with make rpm}}
%{!?tcs_date: %{error: tcs_date is not defined: build with make rpm}}

# The stage is already what is to be installed, stripped or not as
# `make stage` was told by STRIP. RPM's own post-install steps would
# strip it regardless, byte-compile the Python under /opt/tcs, and split
# out debugging information, none of which the .deb has, so they are
# turned off and the two packages hold the same bytes.
%global debug_package %{nil}
%global __os_install_post %{nil}
%global _build_id_links none

Name:		tcslog-gen
Version:	%{tcs_version}
Release:	%{tcs_release}%{?dist}
Summary:	Makes tcslog logs to test with, and tcslog's examples
Group:		Development/Tools
License:	MIT OR Apache-2.0
URL:		https://github.com/vomlehn/tcslog
%global tcs_maintainer David VomLehn <dvomlehn@gmail.com>
Packager:	%{tcs_maintainer}
Recommends:	tcslog, python3
# tcslog-gen and the examples were tcslog's before 0.7.0.
Conflicts:	tcslog < 0.7.0

%description
Tcslog-gen writes chains of tcslog segment files, to test what reads
them; it is a verification tool, and never flies. The package also holds
the examples of tcslog's Rust, C, C++ and Python interfaces, which the
tcslog package's library runs.

%install
cp -a %{tcs_stage}/. %{buildroot}/

# Takes away the links tcslog-gen-links made, in either directory it is
# meant for, while it is still installed, so that removing the package
# leaves none dangling. Only when the package is going ($1 is 0), not on
# an upgrade, and never failing the removal.
%preun
if [ "$1" = 0 ]; then
	for dir in /usr/local/bin /opt/bin; do
		/opt/tcs/sbin/tcslog-gen-links --remove --dir "$dir" \
			> /dev/null 2>&1 || true
	done
fi

%files
%dir /opt/tcs
%dir /opt/tcs/bin
%dir /opt/tcs/sbin
%dir /opt/tcs/share
%dir /opt/tcs/share/doc
%dir /opt/tcs/share/examples
/opt/tcs/bin/tcslog-gen
/opt/tcs/sbin/tcslog-gen-links
%doc /opt/tcs/share/doc/tcslog-gen/
/opt/tcs/share/examples/tcslog/
%config(noreplace) /etc/profile.d/tcslog-gen.sh

# One entry, for the version being built, dated by the Makefile from the
# commit it was built from, so that a new version needs no edit here.
%changelog
* %{tcs_date} %{tcs_maintainer} - %{version}-%{release}
- tcslog-gen %{version}, as released upstream.
