# The tcslog RPM, built by `make rpm` from the tree `make stage`
# installed under build/stage/tcslog. Nothing is compiled here: %install
# copies that tree, so the RPM and the .deb hold the same files, built
# once. The Makefile passes tcs_version, from Cargo.toml; tcs_release,
# the Debian revision, so that the two are named alike; tcs_abi, from
# tcslogc/abi-version, which names the shared library; tcs_stage;
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

Name:		tcslog
Version:	%{tcs_version}
Release:	%{tcs_release}%{?dist}
Summary:	Segmented telemetry log for storage-bounded systems
Group:		Development/Libraries
License:	MIT OR Apache-2.0
URL:		https://github.com/vomlehn/tcslog
%global tcs_maintainer David VomLehn <dvomlehn@gmail.com>
Packager:	%{tcs_maintainer}

%description
Tcslog is flight software that logs telemetry into a directory of
fixed-size segment files and reads it back, recovering from segment
files that were lost or damaged, for vehicles whose contact with the
ground is intermittent.

This package holds the C/C++ library (shared and static), its headers
and pkg-config file, the Python 3 package over it, and the
documentation, all under /opt/tcs: the operational code alone.
tcslog-gen and the examples are in the tcslog-gen package.

%install
cp -a %{tcs_stage}/. %{buildroot}/
# The stage has the .pth where Debian's python3 looks; this one's is
# written when it is installed, by the trigger below.
rm -rf %{buildroot}/usr/lib/python3
# RPM finds what a shared library provides only in an executable file,
# where Debian wants one that is not.
chmod 755 %{buildroot}/opt/tcs/lib/libtcslogc.so.%{tcs_abi}

# Takes away the .pth file the trigger below wrote, only when the package
# is going ($1 is 0), not on an upgrade.
%preun
if [ "$1" = 0 ]; then
	rm -f /usr/lib/python3*/site-packages/tcslog.pth
fi

# The .pth file that puts /opt/tcs/lib/python3/site-packages on python3's
# path. It goes in python3's own site-packages, which on Fedora names its
# minor version (/usr/lib/python3.15/site-packages), so the python3 that is
# installed says where, rather than the one that built the package. This
# runs when the package is installed with a python3 there, and again when
# python3 is installed or upgraded, moving the file to the new version's
# directory; it never fails the transaction.
%triggerin -- python3
rm -f /usr/lib/python3*/site-packages/tcslog.pth
site=$(python3 -c 'import sysconfig
print(sysconfig.get_path("purelib", "posix_prefix", {"base": "/usr"}))') \
	&& mkdir -p "$site" \
	&& echo /opt/tcs/lib/python3/site-packages > "$site/tcslog.pth" || :

# python3 removed ($2 is 0) takes the .pth with it.
%triggerun -- python3
if [ "$2" = 0 ]; then
	rm -f /usr/lib/python3*/site-packages/tcslog.pth
fi

%post -p /sbin/ldconfig
%postun -p /sbin/ldconfig

%files
%dir /opt/tcs
%dir /opt/tcs/include
%dir /opt/tcs/lib
%dir /opt/tcs/lib/pkgconfig
%dir /opt/tcs/lib/python3
%dir /opt/tcs/lib/python3/site-packages
%dir /opt/tcs/share
%dir /opt/tcs/share/doc
/opt/tcs/include/tcslogc.h
/opt/tcs/include/tcslogc.hpp
/opt/tcs/lib/libtcslogc.so
/opt/tcs/lib/libtcslogc.so.%{tcs_abi}
/opt/tcs/lib/libtcslogc.a
/opt/tcs/lib/pkgconfig/tcslogc.pc
/opt/tcs/lib/python3/site-packages/tcslog/
/opt/tcs/lib/python3/site-packages/tcslog-%{version}.dist-info/
%doc /opt/tcs/share/doc/tcslog/
%config(noreplace) /etc/ld.so.conf.d/tcslog.conf
%config(noreplace) /etc/profile.d/tcslog.sh

# One entry, for the version being built, dated by the Makefile from the
# commit it was built from, so that a new version needs no edit here; what
# changed upstream is in /opt/tcs/share/doc/tcslog/CHANGELOG.md.
%changelog
* %{tcs_date} %{tcs_maintainer} - %{version}-%{release}
- tcslog %{version}, as released upstream.
