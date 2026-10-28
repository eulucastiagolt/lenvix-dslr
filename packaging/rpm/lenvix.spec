Name:           lenvix
Version:        0.1.0
Release:        1%{?dist}
Summary:        DSLR & mirrorless live preview, controls and virtual webcam for Linux
License:        GPL-3.0-or-later
URL:            https://github.com/eulucastiagolt/lenvix-dslr

Requires:       gtk4
Requires:       libgphoto2
Recommends:     v4l2loopback

%description
Lenvix turns your DSLR or mirrorless camera into a studio-grade virtual
webcam on Linux using v4l2loopback and libgphoto2, with full camera controls
and dynamic collision avoidance.

%install
mkdir -p %{buildroot}%{_bindir}
mkdir -p %{buildroot}%{_datadir}/applications
mkdir -p %{buildroot}%{_datadir}/icons/hicolor/scalable/apps
mkdir -p %{buildroot}%{_datadir}/metainfo

install -m 755 %{_sourcedir}/lenvix %{buildroot}%{_bindir}/lenvix
install -m 644 %{_sourcedir}/io.github.eulucastiagolt.lenvix.desktop %{buildroot}%{_datadir}/applications/
install -m 644 %{_sourcedir}/io.github.eulucastiagolt.lenvix.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/
install -m 644 %{_sourcedir}/io.github.eulucastiagolt.lenvix.metainfo.xml %{buildroot}%{_datadir}/metainfo/

%files
%{_bindir}/lenvix
%{_datadir}/applications/io.github.eulucastiagolt.lenvix.desktop
%{_datadir}/icons/hicolor/scalable/apps/io.github.eulucastiagolt.lenvix.svg
%{_datadir}/metainfo/io.github.eulucastiagolt.lenvix.metainfo.xml
