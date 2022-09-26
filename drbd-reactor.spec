%define debug_package %{nil}

Name:		drbd-reactor
Version:	0.9.0~rc.3
Release:	1
Summary:	React to DRBD events via plugins.
%global	tarball_version %(echo "%{version}" | sed -e 's/~rc/-rc/' -e 's/~alpha/-alpha/')

Group:		System Environment/Daemons
License:	ASL 2.0
URL:		https://www.github.com/LINBIT/drbd-reactor
Source0:	https://pkg.linbit.com/downloads/drbd/utils/%{name}-%{tarball_version}.tar.gz

BuildRequires:	systemd
# While most pkgs I looked at have bash-completion as BuildRequires, I don't think we need it
# looks like it would only help for cmake or pkgconfig projects.
# BuildRequires:	bash-completion
Requires:	drbd-utils >= 9.21.2
Requires:	python3
# python3{,6}-toml is for rhel7/8 only in epel, but we mirror that pkg
%if 0%{?suse_version} >= 1500 || 0%{?rhel} >= 8
Requires:	python3-toml
%else
%if 0%{?rhel} == 7
Requires:	python36-toml
%endif
%endif

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
install -D -m644 %{_builddir}/%{name}-%{tarball_version}/example/ctl.completion.bash %{buildroot}/%{_datadir}/bash-completion/completions/drbd-reactorctl


%files
# %{_unitdir}/drbd-reactor.service
/lib/systemd/system/drbd-reactor.service
/usr/sbin/drbd-reactor
/usr/sbin/drbd-reactorctl
%{_datadir}/bash-completion/completions/drbd-reactorctl
%{_mandir}/man1/drbd-reactor.1*
%{_mandir}/man1/drbd-reactorctl.1*
%{_mandir}/man5/drbd-reactor.toml.5*
%{_mandir}/man5/drbd-reactor.umh.5*
%{_mandir}/man5/drbd-reactor.promoter.5*
%{_mandir}/man5/drbd-reactor.debugger.5*
%{_mandir}/man5/drbd-reactor.prometheus.5*
%config(noreplace) /etc/drbd-reactor.toml
/etc/drbd-reactor.d
%doc README.md
%doc example/drbd-reactor-reload.path
%doc example/drbd-reactor-reload.service
%doc example/drbd-reactor.toml
%doc example/on-no-quorum-io-error.sh


%changelog
* Mon Sep 26 2022 Roland Kammerer <roland.kammerer@linbit.com> - 0.9.0~rc.3-1
-  New upstream release

* Fri Sep 09 2022 Roland Kammerer <roland.kammerer@linbit.com> - 0.9.0~rc.2-1
-  New upstream release

* Wed Sep 07 2022 Roland Kammerer <roland.kammerer@linbit.com> - 0.9.0~rc.1-1
-  New upstream release

* Tue Jun 28 2022 Roland Kammerer <roland.kammerer@linbit.com> - 0.8.0-1
-  New upstream release

* Fri Jun 10 2022 Roland Kammerer <roland.kammerer@linbit.com> - 0.8.0~rc.1-1
-  New upstream release

* Thu May 19 2022 Roland Kammerer <roland.kammerer@linbit.com> - 0.7.0-1
-  New upstream release

* Thu May 12 2022 Roland Kammerer <roland.kammerer@linbit.com> - 0.7.0~rc.1-1
-  New upstream release

* Thu Apr 28 2022 Roland Kammerer <roland.kammerer@linbit.com> - 0.6.1-1
-  New upstream release

* Mon Apr 04 2022 Roland Kammerer <roland.kammerer@linbit.com> - 0.6.0-1
-  New upstream release

* Mon Jan 31 2022 Roland Kammerer <roland.kammerer@linbit.com> - 0.5.3-1
-  New upstream release

* Mon Jan 17 2022 Roland Kammerer <roland.kammerer@linbit.com> - 0.5.2-1
-  New upstream release

* Tue Nov 30 2021 Roland Kammerer <roland.kammerer@linbit.com> - 0.5.1-1
-  New upstream release

* Fri Nov 19 2021 Roland Kammerer <roland.kammerer@linbit.com> - 0.5.0-1
-  New upstream release

* Wed Nov 10 2021 Roland Kammerer <roland.kammerer@linbit.com> - 0.5.0~rc.1-1
-  New upstream release

* Tue Aug 10 2021 Roland Kammerer <roland.kammerer@linbit.com> - 0.4.4-1
-  New upstream release

* Mon Aug 02 2021 Roland Kammerer <roland.kammerer@linbit.com> - 0.4.3-1
-  New upstream release

* Tue Jul 27 2021 Roland Kammerer <roland.kammerer@linbit.com> - 0.4.2-1
-  New upstream release

* Wed Jul 21 2021 Roland Kammerer <roland.kammerer@linbit.com> - 0.4.1-1
-  New upstream release

* Fri Jun 18 2021 Roland Kammerer <roland.kammerer@linbit.com> - 0.4.0-1
-  New upstream release

* Fri Jun 11 2021 Roland Kammerer <roland.kammerer@linbit.com> - 0.4.0~rc.2-1
-  New upstream release

* Tue Jun 01 2021 Roland Kammerer <roland.kammerer@linbit.com> - 0.4.0~rc.1-1
-  New upstream release

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
