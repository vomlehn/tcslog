# The tcslog RPM, built by `make rpm` from the tree `make stage`
# installed under build/stage/tcslog. Nothing is compiled here: %install
# copies that tree, so the RPM and the .deb hold the same files, built
# once. The Makefile passes tcs_version, from Cargo.toml; tcs_release,
# the Debian revision, so that the two are named alike; tcs_abi, from
# tcslog-c/abi-version, which names the shared library; tcs_stage;
# tcs_date, the date of the commit built, for the %changelog; and
# tcs_pysite and tcs_pyversion, the site-packages directory and version
# of the python3 it found, which on Fedora are what its python3_sitelib
# and python3_version macros say.

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
# The .pth file that puts /opt/tcs on Python's path is in the directory
# of one Python version, which is the one this RPM was built for. A
# Recommends rather than a Requires, since the C/C++ library is of use
# without any Python at all.
Recommends:	python(abi) = %{tcs_pyversion}

%description
Tcslog is flight software that logs telemetry into a directory of
fixed-size segment files and reads it back, recovering from segment
files that were lost or damaged, for vehicles whose contact with the
ground is intermittent.

This package holds the C/C++ library (shared and static), its headers
and pkg-config file, the Python 3 package over it, tcslog-gen, and the
documentation, all under /opt/tcs.

%install
cp -a %{tcs_stage}/. %{buildroot}/
# The stage has the .pth where Debian's python3 looks; this one looks in
# its own site-packages.
pth=%{buildroot}/usr/lib/python3/dist-packages/tcslog.pth
if [ "%{tcs_pysite}" != /usr/lib/python3/dist-packages ]; then
	mkdir -p %{buildroot}%{tcs_pysite}
	mv $pth %{buildroot}%{tcs_pysite}/
	rmdir %{buildroot}/usr/lib/python3/dist-packages
	rmdir %{buildroot}/usr/lib/python3
fi
# RPM finds what a shared library provides only in an executable file,
# where Debian wants one that is not.
chmod 755 %{buildroot}/opt/tcs/lib/libtcslog.so.%{tcs_abi}

# Takes away the links tcslog-links made, in either directory it is
# meant for, while it is still installed, so that removing the package
# leaves none dangling. Only when the package is going ($1 is 0), not on
# an upgrade, and never failing the removal.
%preun
if [ "$1" = 0 ]; then
	for dir in /usr/local/bin /opt/bin; do
		/opt/tcs/sbin/tcslog-links --remove --dir "$dir" \
			> /dev/null 2>&1 || true
	done
fi

%post -p /sbin/ldconfig
%postun -p /sbin/ldconfig

%files
%dir /opt/tcs
%dir /opt/tcs/bin
%dir /opt/tcs/sbin
%dir /opt/tcs/include
%dir /opt/tcs/lib
%dir /opt/tcs/lib/pkgconfig
%dir /opt/tcs/lib/python3
%dir /opt/tcs/lib/python3/site-packages
%dir /opt/tcs/share
%dir /opt/tcs/share/doc
%dir /opt/tcs/share/examples
/opt/tcs/bin/tcslog-gen
/opt/tcs/sbin/tcslog-links
/opt/tcs/include/tcslog.h
/opt/tcs/include/tcslog.hpp
/opt/tcs/lib/libtcslog.so
/opt/tcs/lib/libtcslog.so.%{tcs_abi}
/opt/tcs/lib/libtcslog.a
/opt/tcs/lib/pkgconfig/tcslog.pc
/opt/tcs/lib/python3/site-packages/tcslog/
/opt/tcs/lib/python3/site-packages/tcslog-%{version}.dist-info/
%doc /opt/tcs/share/doc/tcslog/
/opt/tcs/share/examples/tcslog/
%config(noreplace) /etc/ld.so.conf.d/tcslog.conf
%config(noreplace) /etc/profile.d/tcslog.sh
%{tcs_pysite}/tcslog.pth

# One entry, for the version being built, dated by the Makefile from the
# commit it was built from, so that a new version needs no edit here; what
# changed upstream is in /opt/tcs/share/doc/tcslog/CHANGELOG.md.
%changelog
* %{tcs_date} %{tcs_maintainer} - %{version}-%{release}
- tcslog %{version}, as released upstream.
