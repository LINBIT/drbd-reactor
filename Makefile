PROG := drbd-reactor
DEBUG ?=
DESTDIR =
DEBCONTAINER=drbd-reactor:deb
RPMCONTAINER=drbd-reactor:rpm
REL = $(PROG)-$(VERSION)
MANPAGES = $(wildcard doc/*.1) $(wildcard doc/*.5)

DOCKERREGISTRY := drbd.io
ARCH ?= amd64
ifneq ($(strip $(ARCH)),)
DOCKERREGISTRY := $(DOCKERREGISTRY)/$(ARCH)
endif
DOCKERREGPATH = $(DOCKERREGISTRY)/$(PROG)
DOCKER_TAG ?= latest


ifneq ($(wildcard vendor/.),)
OFFLINE = --offline
endif

# don't use info as this prints to stdout which messes up 'dockerpath' target
$(shell echo DEBUG is $(DEBUG) >&2)
$(shell echo OFFLINE is $(OFFLINE) >&2)

ifdef DEBUG
	RELEASE :=
	TARGET := debug
else
	RELEASE := --release
	TARGET := release
endif

build: ## cargo build binary
	cargo build $(OFFLINE) $(RELEASE)

.PHONY: help
help:
		@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-30s\033[0m %s\n", $$1, $$2}'

.PHONY: debcontainer
debcontainer: ## build docker container for deb packaging
	cd docker && docker build -t $(DEBCONTAINER) -f Dockerfile-deb .

.PHONY: rpmcontainer
rpmcontainer: ## build docker container for rpm packaging
	cd docker && docker build -t $(RPMCONTAINER) -f Dockerfile-rpm .

.PHONY: deb
deb: ## Build a deb package
	tmpdir=$$(mktemp -d) && \
	docker run -it --rm -v $$PWD:/src:ro -v $$tmpdir:/out --entrypoint=/src/docker/entry.sh $(DEBCONTAINER) deb && \
	mv $$tmpdir/*.deb . && echo "rm -rf $$tmpdir"

.PHONY: rpm
rpm: ## Build a rpm package
	tmpdir=$$(mktemp -d) && \
	docker run -it --rm -v $$PWD:/src:ro -v $$tmpdir:/out --entrypoint=/src/docker/entry.sh $(RPMCONTAINER) rpm && \
	mv $$tmpdir/*.rpm . && echo "rm -rf $$tmpdir"

.PHONY: tabcompletion
tabcompletion: ## Build tab completions in drbd-reactor:deb
	tmpdir=$$(mktemp -d) && \
	docker run -it --rm -v $$PWD:/src:ro -v $$tmpdir:/out --entrypoint=/src/docker/entry.sh $(DEBCONTAINER) tabcompletion && \
	mv $$tmpdir/*.completion.* ./example/ && echo "rm -rf $$tmpdir"

install:  # install binary and config
	install -D -m 0750 target/$(TARGET)/$(PROG) $(DESTDIR)/usr/sbin/$(PROG)
	install -D -m 0750 $(PROG)ctl.py $(DESTDIR)/usr/sbin/$(PROG)ctl
	install -D -m 0640 example/drbd-reactor.toml $(DESTDIR)/etc/drbd-reactor.toml
	install -d -m 0750 $(DESTDIR)/etc/drbd-reactor.d
	install -D -m 0644 example/drbd-reactor.service $(DESTDIR)/lib/systemd/system/drbd-reactor.service
	for f in $(MANPAGES); do \
		sect=$$(echo $$f | sed -e 's/.*\.\([0-9]\)$$/\1/'); \
		install -D -m 0644 $$f $(DESTDIR)/usr/share/man/man$${sect}/$$(basename $$f); \
	done

clean: ## cargo clean
	cargo clean

test: ## cargo test
	cargo test

debrelease: checkVERSION
	rm -rf .debrelease && mkdir .debrelease
	cd .debrelease && git clone $(PWD) . && \
	mkdir .cargo && cp vendor.toml .cargo/config && \
	rm -rf vendor && cargo vendor && rm -fr vendor/winapi*gnu*/lib/*.a && \
	tar --owner=0 --group=0 --transform 's,^,$(REL)/,' -czf ../$(REL).tar.gz \
		$$(git ls-files | grep -v '^\.') .cargo/config vendor
	rm -rf .debrelease

release: checkVERSION
	tar --owner=0 --group=0 --transform 's,^,$(REL)/,' -czf $(REL).tar.gz \
		$$(git ls-files | grep -v '^\.' | grep -v '^debian\/')

ifndef VERSION
checkVERSION:
	$(error environment variable VERSION is not set)
else
checkVERSION:
	test -z "$$(git ls-files -m)"
	lbvers.py check --base=$(BASE) --build=$(BUILD) --build-nr=$(BUILD_NR) --pkg-nr=$(PKG_NR) \
		--cargo=Cargo.toml --debian-changelog=debian/changelog --rpm-spec=drbd-reactor.spec
	if test $$(grep "ENV DRBD_REACTOR_VERSION $(VERSION)" Dockerfile | wc -l) -ne 2; then \
		echo -e "\n\tDockerfile needs update"; \
	false; fi;
endif

.PHONY: dockerimage
dockerimage:
	docker build -t $(DOCKERREGPATH):$(DOCKER_TAG) $(EXTRA_DOCKER_BUILDARGS) .
	docker tag $(DOCKERREGPATH):$(DOCKER_TAG) $(DOCKERREGPATH):latest

.PHONY: dockerpath
dockerpath:
	@echo $(DOCKERREGPATH):latest $(DOCKERREGPATH):$(DOCKER_TAG)
