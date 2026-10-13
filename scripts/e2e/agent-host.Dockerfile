# A throwaway "podman host": Fedora with systemd as PID 1, podman, git.
# Runs privileged under docker so quadlet + systemd + podman work without root on the docker host.
FROM registry.fedoraproject.org/fedora:41
RUN dnf -y install systemd podman fuse-overlayfs git iproute procps-ng && dnf clean all
RUN mkdir -p /etc/containers && printf '[storage]\ndriver = "overlay"\n[storage.options.overlay]\nmount_program = "/usr/bin/fuse-overlayfs"\n' > /etc/containers/storage.conf
# The agent maps each unit onto a fixed slice of this range, owned by the `containers` user.
RUN echo 'containers:2147483647:2147483648' | tee -a /etc/subuid /etc/subgid >/dev/null
# A rootless user for the rootless agent; its default range plus a 64-slice one added after it.
RUN useradd -m alice && usermod --add-subuids 1000000-5194303 --add-subgids 1000000-5194303 alice
# Nested in docker, rootless podman cannot mount its default pod infra rootfs
# (an overlay from /run/user); a pause image needs no such mount.
RUN mkdir -p /home/alice/.config/containers && printf '[engine]\ninfra_image = "registry.k8s.io/pause:3.10"\n' > /home/alice/.config/containers/containers.conf && chown -R alice:alice /home/alice/.config
# The image layers drop file capabilities; rootless podman needs these two.
RUN setcap cap_setuid+ep /usr/bin/newuidmap && setcap cap_setgid+ep /usr/bin/newgidmap
# logind stays: it starts a lingering user's systemd at boot.
RUN systemctl mask systemd-udevd.service systemd-udev-trigger.service getty.target console-getty.service
STOPSIGNAL SIGRTMIN+3
CMD ["/sbin/init"]
