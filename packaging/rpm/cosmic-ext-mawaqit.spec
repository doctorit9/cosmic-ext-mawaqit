Name:           cosmic-ext-mawaqit
Version:        0.1.0
Release:        1%{?dist}
Summary:        Islamic prayer times applet for COSMIC Desktop

License:        MIT
URL:            https://github.com/doctorit9/cosmic-ext-mawaqit
Source0:        %{url}/archive/refs/tags/v%{version}.tar.gz

BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  just
BuildRequires:  pkgconfig(wayland-client)
BuildRequires:  pkgconfig(xkbcommon)
BuildRequires:  pkgconfig(alsa)
BuildRequires:  openssl-devel

Requires:       cosmic-panel

%description
A native prayer times applet for the COSMIC Desktop written in Rust.
Features live countdown, authentic Adhan audio alerts, offline astronomical
calculations, and full Arabic/English localization.

%prep
%autosetup -n %{name}-%{version}

%build
just build-release

%install
just rootdir=%{buildroot} prefix=%{_prefix} install

%files
%{_bindir}/mawaqit
%{_datadir}/applications/io.github.doctorit.mawaqit.desktop
%{_datadir}/appdata/io.github.doctorit.mawaqit.metainfo.xml
%{_datadir}/icons/hicolor/scalable/apps/io.github.doctorit.mawaqit.svg

%changelog
* Thu Nov 19 2026 doctorit9 <https://github.com/doctorit9> - 0.1.0-1
- Initial packaging release for COSMIC Desktop
