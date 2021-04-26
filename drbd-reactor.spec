%define debug_package %{nil}

Name:		drbd-reactor
Version:	0.3.0
Release:	1
Summary:	React to DRBD events via plugins.
%global	tarball_version %(echo "%{version}" | sed -e 's/~rc/-rc/' -e 's/~alpha/-alpha/')

Group:		System Environment/Daemons
License:	ASL 2.0
URL:		https://www.github.com/LINBIT/drbd-reactor
Source0:	https://www.linbit.com/downloads/drbd/utils/%{name}-%{tarball_version}.tar.gz

BuildRequires:	systemd
Requires:	drbd-utils >= 9.17.0

%description
Daemon monitoring the state of DRBD resources, and executing plugins
acting on state changes.
Plugins can for example monitor resources or promote DRBD resources.

%prep
%setup -q -n %{name}-%{tarball_version}


%build
make %{?_smp_mflags}


%install
make install DESTDIR=%{buildroot}


%files
# %{_unitdir}/drbd-reactor.service
/lib/systemd/system/drbd-reactor.service
/usr/sbin/drbd-reactor
%{_mandir}/man1/drbd-reactor.1*
%{_mandir}/man5/drbd-reactor.toml.5*
%{_mandir}/man5/drbd-reactor.umh.5*
%{_mandir}/man5/drbd-reactor.promoter.5*
%{_mandir}/man5/drbd-reactor.debugger.5*
%{_mandir}/man5/drbd-reactor.prometheus.5*
%config(noreplace) /etc/drbd-reactor.toml
%doc README.md


%changelog
* Mon Apr 26 2021 Roland Kammerer <roland.kammerer@linbit.com> - 0.3.0-1
-  New upstream release

* Tue Apr 20 2021 Roland Kammerer <roland.kammerer@linbit.com> - 0.3.0~rc.1-1
-  New upstream release

* Tue Mar 23 2021 Roland Kammerer <roland.kammerer@linbit.com> - 0.2.0-1
-  New upstream release

* Tue Mar 16 2021 Roland Kammerer <roland.kammerer@linbit.com> - 0.2.0~rc.1-1
-  New upstream release

* Fri Feb 26 2021 Roland Kammerer <roland.kammerer@libit.com> - 0.1.0-1
-  New upstream release

* Sat Feb 20 2021 Roland Kammerer <roland.kammerer@linbit.com> - 0.1.0~rc.2-1
-  New upstream release

* Wed Feb 17 2021 Roland Kammerer <roland.kammerer@linbit.com> - 0.1.0~rc.1-1
-  New upstream release
