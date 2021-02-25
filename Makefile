PROG := drbdd
DEBUG ?=
DESTDIR =
DEBCONTAINER=drbdd:deb
RPMCONTAINER=drbdd:rpm
REL = $(PROG)-$(VERSION)

ifneq ($(wildcard vendor/.),)
OFFLINE = --offline
endif

$(info DEBUG is $(DEBUG))
$(info OFFLINE is $(OFFLINE))

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
	cd docker && docker build -t $(DEBCONTAINER) -f Dockerfile.debian .

.PHONY: rpmcontainer
rpmcontainer: ## build docker container for rpm packaging
	cd docker && docker build -t $(RPMCONTAINER) -f Dockerfile.centos .

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

install:  # install binary and config
	install -D -m 0750 target/$(TARGET)/$(PROG) $(DESTDIR)/usr/sbin/$(PROG)
	install -D -m 0640 example/drbdd.toml $(DESTDIR)/etc/drbdd.toml
	install -D -m 0640 example/drbdd.service $(DESTDIR)/lib/systemd/system/drbdd.service

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
	lbvers.py check --base=$(BASE) --build=$(BUILD) --build-nr=$(BUILD_NR) --pkg-nr=$(PKG_NR) \
		--cargo=Cargo.toml --debian-changelog=debian/changelog --rpm-spec=drbdd.spec
endif
