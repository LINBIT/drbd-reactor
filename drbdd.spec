%define debug_package %{nil}

Name:		drbdd
Version:	0.2.0
Release:	1
Summary:	Monitors DRBD resources via plugins.
%global	tarball_version %(echo "%{version}" | sed -e 's/~rc/-rc/' -e 's/~alpha/-alpha/')

Group:		System Environment/Daemons
License:	ASL 2.0
URL:		https://www.github.com/LINBIT/drbdd
Source0:	https://www.linbit.com/downloads/drbd/utils/%{name}-%{tarball_version}.tar.gz

BuildRequires:	systemd

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
# %{_unitdir}/drbdd.service
/lib/systemd/system/drbdd.service
/usr/sbin/drbdd
%{_mandir}/man1/drbdd.1*
%{_mandir}/man5/drbdd.toml.5*
%{_mandir}/man5/drbdd.umh.5*
%{_mandir}/man5/drbdd.promoter.5*
%{_mandir}/man5/drbdd.debugger.5*
%config(noreplace) /etc/drbdd.toml
%doc README.md


%changelog
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
