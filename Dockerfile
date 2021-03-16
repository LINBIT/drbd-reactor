FROM registry.access.redhat.com/ubi7/ubi as builder

ENV DRBDD_VERSION 0.2.0-rc.1

ENV DRBDD_TGZNAME drbdd
ENV DRBDD_TGZ ${DRBDD_TGZNAME}-${DRBDD_VERSION}.tar.gz

# need to setup our own toolchain to cover archs not in rust:lastest
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
	sh -s -- --profile minimal -y -q --no-modify-path

RUN yum -y update-minimal --security --sec-severity=Important --sec-severity=Critical && \
	yum install -y gcc wget && \
	yum clean all -y

# one can not comment COPY
RUN cd /tmp && wget https://www.linbit.com/downloads/drbd/utils/${DRBDD_TGZ} # !lbbuild
# =lbbuild COPY /${DRBDD_TGZ} /tmp/

RUN cd /tmp && tar xvf ${DRBDD_TGZ} && cd ${DRBDD_TGZNAME}-${DRBDD_VERSION} && \
	. $HOME/.cargo/env; cargo install --path . && \
	cp $HOME/.cargo/bin/drbdd /tmp && \
	cp ./example/drbdd.toml /tmp

FROM quay.io/linbit/drbd-utils
MAINTAINER Roland Kammerer <roland.kammerer@linbit.com>

ENV DRBDD_VERSION 0.2.0-rc.1

ARG release=1
LABEL	name="drbdd" \
	vendor="LINBIT" \
	version="$DRBDD_VERSION" \
	release="$release" \
	summary="DRBD monitoring via plugins" \
	description="DRBD monitoring via plugins"

COPY COPYING /licenses/Apache-2.0.txt

COPY --from=builder /tmp/drbdd /usr/sbin
COPY --from=builder /tmp/drbdd.toml /etc

RUN yum -y update-minimal --security --sec-severity=Important --sec-severity=Critical && \
	yum clean all -y

ENTRYPOINT ["/usr/sbin/drbdd"]
