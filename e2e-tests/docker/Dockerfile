FROM ubuntu:22.04

ARG LBPYTEST_VERSION=0.2.1

RUN apt-get update && \
    apt-get -y install \
        python3 \
        python3-pip \
        openssh-client && \
    apt-get clean && rm -rf /var/lib/apt/lists/*

RUN pip3 install lbpytest==${LBPYTEST_VERSION} toml==0.10.2

COPY entry.sh /

ENV TEST_NAME=
ENV TARGETS=

ENTRYPOINT [ "/entry.sh" ]
