#!/usr/bin/env bash

set -eu

die() {
    >&2 echo "$1"
    exit 1
}

help() {
    cat <<EOF
Usage:
 $(basename "$0") linstor_db_backing_device <backing_dev_fs>

Start a temporary LINSTOR controller with a RO DB from a backing device.
EOF
    exit "$1"
}

checks() {
    local backing_dev drbd_res
    backing_dev="$1"
    drbd_res="$(basename "$backing_dev" _00000)"

    [[ $(id -u) == 0 ]] || die "needs to be executed as root"
    drbdadm role "$drbd_res" &>/dev/null && die "make sure the DRBD resource is down on all nodes"
    systemctl -q list-unit-files linstor-controller.service &>/dev/null || die "LINSTOR controller (i.e., linstor-controller.service) missing"
    modprobe overlay &>/dev/null || true
    grep -q overlay /proc/filesystems || die "overlayfs not supported by kernel"
    findmnt "$backing_dev" && die "backing device ('${backing_dev}') already mounted"
    [[ -d /var/lib/linstor ]] || die "/var/lib/linstor does not exist"
}

handle_reactor_override() {
    local override_dir="/run/systemd/system/linstor-controller.service.d"

    if [[ -d $override_dir ]]; then
        mv "$override_dir" "${override_dir}.bak"
        systemctl daemon-reload
    elif [[ -d ${override_dir}.bak ]]; then
        mv "${override_dir}.bak" "${override_dir}"
        systemctl daemon-reload
    fi

    return 0
}

# main
[ "$#" -eq 1 ] || [ "$#" -eq 2 ] || help 1
backing_dev="$1"
checks "$backing_dev"

backing_dev_fs="ext4"
[ "$#" -eq 2 ] && backing_dev_fs="$2"

TMPDIR="$(mktemp -d)"
for d in lower upper work; do
    mkdir -p "${TMPDIR}/$d"
done

echo "temporary mount points are below ${TMPDIR}"

echo "mounting backing device ${backing_dev}"
mount -o ro -t "$backing_dev_fs" "$backing_dev" "${TMPDIR}/lower/"
echo "mounting overlay directory /var/lib/linstor/"
mount -t overlay overlay -o "lowerdir=${TMPDIR}/lower,upperdir=${TMPDIR}/upper,workdir=${TMPDIR}/work" /var/lib/linstor/
handle_reactor_override
echo "starting LINSTOR controller"
systemctl start linstor-controller.service
echo "[OK] LINSTOR controller running"

ctrl_c() {
    echo "cleaning up"
    echo "stopping LINSTOR controller"
    systemctl stop linstor-controller.service
    handle_reactor_override
    echo "umounting overlay directory /var/lib/linstor/"
    umount /var/lib/linstor
    echo "umounting backing device ${backing_dev}"
    umount "${backing_dev}"

    exit
}

trap ctrl_c INT
echo "hit ctrl-c to stop the controller and clean up"
while true; do read -rp ""; done
