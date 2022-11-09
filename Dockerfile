ARG BUILDER=registry.access.redhat.com/ubi8/ubi
FROM $BUILDER as builder

ENV DRBD_REACTOR_VERSION 0.10.0-rc.1

ENV DRBD_REACTOR_TGZNAME drbd-reactor
ENV DRBD_REACTOR_TGZ ${DRBD_REACTOR_TGZNAME}-${DRBD_REACTOR_VERSION}.tar.gz

USER root
RUN yum -y update-minimal --security --sec-severity=Important --sec-severity=Critical && yum install -y cargo rust wget && yum clean all -y # !lbbuild

# one can not comment COPY
RUN cd /tmp && wget https://pkg.linbit.com/downloads/drbd/utils/${DRBD_REACTOR_TGZ} # !lbbuild
# =lbbuild COPY /${DRBD_REACTOR_TGZ} /tmp/

# =lbbuild USER makepkg
RUN test -f $HOME/.cargo/env || install -D /dev/null $HOME/.cargo/env
RUN cd /tmp && tar xvf ${DRBD_REACTOR_TGZ} && cd ${DRBD_REACTOR_TGZNAME}-${DRBD_REACTOR_VERSION} && \
	. $HOME/.cargo/env && cargo install --path . --bin drbd-reactor && \
	cp $HOME/.cargo/bin/drbd-reactor /tmp && \
	cp ./example/drbd-reactor.toml /tmp

FROM quay.io/linbit/drbd-utils
MAINTAINER Roland Kammerer <roland.kammerer@linbit.com>

ENV DRBD_REACTOR_VERSION 0.10.0-rc.1

ARG release=1
LABEL	name="drbd-reactor" \
	vendor="LINBIT" \
	version="$DRBD_REACTOR_VERSION" \
	release="$release" \
	summary="DRBD events reaction via plugins" \
	description="DRBD events reaction via plugins"

COPY COPYING /licenses/Apache-2.0.txt

COPY --from=builder /tmp/drbd-reactor /usr/sbin
COPY --from=builder /tmp/drbd-reactor.toml /etc

RUN yum -y update-minimal --security --sec-severity=Important --sec-severity=Critical && \
	yum clean all -y

ENTRYPOINT ["/usr/sbin/drbd-reactor"]
