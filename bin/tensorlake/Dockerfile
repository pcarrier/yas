FROM ubuntu:26.04 AS xwayland-satellite-build

ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential ca-certificates cargo clang curl libclang-dev pkg-config \
    libxcb1-dev libxcb-composite0-dev libxcb-cursor-dev libxcb-randr0-dev libxcb-res0-dev \
    && rm -rf /var/lib/apt/lists/*

# Build the pinned upstream release; keep the Rust toolchain out of the final image.
WORKDIR /build/xwayland-satellite
RUN curl -fsSLo /tmp/xwayland-satellite.tar.gz \
      https://codeload.github.com/Supreeeme/xwayland-satellite/tar.gz/refs/tags/v0.8.2 \
    && echo 'cb50bb6948582d5ec3aa511d2d66ad622989bb14bef94e3bb81bae8b64c120b1  /tmp/xwayland-satellite.tar.gz' | sha256sum -c - \
    && tar -xzf /tmp/xwayland-satellite.tar.gz --strip-components=1 \
    && cargo build --locked --release --bin xwayland-satellite \
    && install -Dm755 target/release/xwayland-satellite /out/xwayland-satellite

FROM ubuntu:26.04

ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl htop nodejs npm dbus systemd \
    fonts-dejavu-core fonts-noto-color-emoji libpipewire-0.3-0 pipewire \
    pipewire-pulse wireplumber mesa-vulkan-drivers libvulkan1 libegl1 libgles2 libopengl0 \
    xwayland libxcb-composite0 libxcb-cursor0 libxcb-randr0 libxcb-res0 \
    && rm -rf /var/lib/apt/lists/*

# Tensorlake injects matching NVIDIA driver libraries, but not their loader
# registrations. libEGL.so.1 and the EGL vendor registration are also needed
# by NVIDIA's Vulkan ICD, even for the headless compositor.
RUN install -d /usr/share/vulkan/icd.d /usr/share/glvnd/egl_vendor.d \
    && printf '%s\n' \
      '{"file_format_version":"1.0.0","ICD":{"library_path":"libGLX_nvidia.so.0","api_version":"1.3.0"}}' \
      > /usr/share/vulkan/icd.d/nvidia_icd.json \
    && printf '%s\n' \
      '{"file_format_version":"1.0.0","ICD":{"library_path":"libEGL_nvidia.so.0"}}' \
      > /usr/share/glvnd/egl_vendor.d/10_nvidia.json

COPY --from=xwayland-satellite-build /out/xwayland-satellite /usr/local/bin/xwayland-satellite
RUN xwayland-satellite -version && Xwayland -version

# Install native browser packages from the upstream repositories.
RUN install -d -m 0755 /etc/apt/keyrings \
    && curl -fsSLo /usr/share/keyrings/brave-browser-archive-keyring.gpg \
      https://brave-browser-apt-release.s3.brave.com/brave-browser-archive-keyring.gpg \
    && curl -fsSLo /etc/apt/sources.list.d/brave-browser-release.sources \
      https://brave-browser-apt-release.s3.brave.com/brave-browser.sources \
    && curl -fsSLo /etc/apt/keyrings/packages.mozilla.org.asc \
      https://packages.mozilla.org/apt/repo-signing-key.gpg \
    && printf '%s\n' \
      'Types: deb' 'URIs: https://packages.mozilla.org/apt' \
      'Suites: mozilla' 'Components: main' \
      'Signed-By: /etc/apt/keyrings/packages.mozilla.org.asc' \
      > /etc/apt/sources.list.d/mozilla.sources \
    && printf '%s\n' \
      'Package: *' 'Pin: origin packages.mozilla.org' 'Pin-Priority: 1000' \
      '' 'Package: firefox' 'Pin: release o=Ubuntu' 'Pin-Priority: -1' \
      > /etc/apt/preferences.d/mozilla \
    && apt-get update && apt-get install -y --no-install-recommends brave-browser firefox \
    && test -x /usr/bin/brave-browser && test -x /usr/bin/firefox \
    && rm -rf /var/lib/apt/lists/*

COPY extensions/ /opt/yas/extensions/
COPY install-extensions.mjs /usr/local/lib/yas/install-extensions.mjs
COPY yas /usr/local/bin/yas
RUN chmod 755 /usr/local/bin/yas && /usr/local/bin/yas --version \
    && useradd --create-home --shell /bin/bash yas \
    && install -d -o yas -g yas -m 0700 /run/yas \
    && install -d -m 0700 /etc/yas \
    && rm -f /sbin/init

ENV XDG_RUNTIME_DIR=/run/yas YAS_SOCK=/run/yas/yas-default.sock MOZ_ENABLE_WAYLAND=1
USER root
WORKDIR /home/yas
# Tensorlake supplies PID 1. The launcher starts managed YAS processes.
ENTRYPOINT ["/bin/sleep", "infinity"]
