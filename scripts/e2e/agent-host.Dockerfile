# A throwaway "podman host": Fedora with systemd as PID 1, podman, git.
# Runs privileged under docker so quadlet + systemd + podman work without root on the docker host.
FROM registry.fedoraproject.org/fedora:41
RUN dnf -y install systemd podman fuse-overlayfs git iproute procps-ng && dnf clean all
RUN mkdir -p /etc/containers && printf '[storage]\ndriver = "overlay"\n[storage.options.overlay]\nmount_program = "/usr/bin/fuse-overlayfs"\n' > /etc/containers/storage.conf
RUN systemctl mask systemd-udevd.service systemd-udev-trigger.service systemd-logind.service getty.target console-getty.service
STOPSIGNAL SIGRTMIN+3
CMD ["/sbin/init"]
