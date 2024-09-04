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

build: ## cargo build binaries
	cargo build $(OFFLINE) $(RELEASE)

.PHONY: help
help:
		@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-30s\033[0m %s\n", $$1, $$2}'

.PHONY: tabcompletion
tabcompletion: ## Build tab completions
	for shell in zsh bash; \
		do cargo run --bin drbd-reactorctl generate-completion $$shell > ./example/ctl.completion.$$shell; \
	done

install:  # install binary and config
	install -D -m 0750 target/$(TARGET)/$(PROG) $(DESTDIR)/usr/sbin/$(PROG)
	install -D -m 0750 target/$(TARGET)/$(PROG)ctl $(DESTDIR)/usr/sbin/$(PROG)ctl
	install -D -m 0750 target/$(TARGET)/ocf-rs-wrapper $(DESTDIR)/usr/libexec/drbd-reactor/ocf-rs-wrapper
	install -D -m 0640 example/drbd-reactor.toml $(DESTDIR)/etc/drbd-reactor.toml
	install -d -m 0750 $(DESTDIR)/etc/drbd-reactor.d
	install -D -m 0644 example/drbd-reactor.service $(DESTDIR)/lib/systemd/system/drbd-reactor.service
	install -D -m 0644 example/ocf.rs@.service $(DESTDIR)/lib/systemd/system/ocf.rs@.service
	for f in $(MANPAGES); do \
		sect=$$(echo $$f | sed -e 's/.*\.\([0-9]\)$$/\1/'); \
		install -D -m 0644 $$f $(DESTDIR)/usr/share/man/man$${sect}/$$(basename $$f); \
	done

clean: ## cargo clean
	cargo clean

test: ## cargo test
	cargo test

sbom/drbd-reactor.cdx.json: Cargo.toml Cargo.lock
	test -d sbom || mkdir sbom
	cargo sbom --output-format cyclone_dx_json_1_4 > $@

sbom/drbd-reactor.spdx.json: Cargo.toml Cargo.lock
	test -d sbom || mkdir sbom
	cargo sbom --output-format spdx_json_2_3 > $@

check-vulns: sbom/drbd-reactor.cdx.json
	osv-scanner --sbom=$<

debrelease: checkVERSION
	rm -rf .debrelease && mkdir .debrelease
	cd .debrelease && git clone $(PWD) . && \
	mkdir .cargo && cp vendor.toml .cargo/config && \
	rm -rf vendor && cargo vendor && rm -fr vendor/winapi*gnu*/lib/*.a && \
	tar --owner=0 --group=0 --transform 's,^,$(REL)/,' -czf ../$(REL).tar.gz \
		$$(git ls-files | grep -v '^\.') .cargo/config vendor
	rm -rf .debrelease

release: checkVERSION sbom/drbd-reactor.cdx.json sbom/drbd-reactor.spdx.json
	tar --owner=0 --group=0 --transform 's,^,$(REL)/,' -czf $(REL).tar.gz \
		$$(git ls-files | grep -v '^\.' | grep -v '^debian/') \
		sbom/drbd-reactor.cdx.json sbom/drbd-reactor.spdx.json

ifndef VERSION
checkVERSION:
	$(error environment variable VERSION is not set)
else
checkVERSION:
	test -z "$$(git ls-files -m)"
	lbvers.py check --base=$(BASE) --build=$(BUILD) --build-nr=$(BUILD_NR) --pkg-nr=$(PKG_NR) \
		--cargo=Cargo.toml --debian-changelog=debian/changelog --rpm-spec=drbd-reactor.spec \
		--dockerfiles=Dockerfile --dockertoken=DRBD_REACTOR_VERSION
endif

.PHONY: dockerimage
dockerimage:
	docker build -t $(DOCKERREGPATH):$(DOCKER_TAG) $(EXTRA_DOCKER_BUILDARGS) .
	docker tag $(DOCKERREGPATH):$(DOCKER_TAG) $(DOCKERREGPATH):latest

.PHONY: dockerpath
dockerpath:
	@echo $(DOCKERREGPATH):latest $(DOCKERREGPATH):$(DOCKER_TAG)
