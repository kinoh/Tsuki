FROM mcr.microsoft.com/devcontainers/rust:1-1-bookworm AS final

RUN rustup toolchain install nightly \
    && rustup override set nightly \
    && rustup component add rustfmt \
    && rustup component add rust-src

RUN curl -sSLf -o /tmp/vosk.zip https://github.com/alphacep/vosk-api/releases/download/v0.3.45/vosk-linux-$(arch)-0.3.45.zip \
    && unzip -qj -d /tmp /tmp/vosk.zip vosk-linux-x86_64-0.3.45/libvosk.so \
    && install /tmp/libvosk.so /usr/local/lib \
    && rm /tmp/vosk.zip /tmp/libvosk.so

RUN curl -sSLf -o /tmp/pnpm https://github.com/pnpm/pnpm/releases/download/v10.6.3/pnpm-linux-x64 \
    && chmod +x /tmp/pnpm \
    && ENV="/home/vscode/.bashrc" SHELL="/usr/bin/bash" /tmp/pnpm setup \
    && rm /tmp/pnpm

RUN apt-get update \
    && apt-get -y install distro-info \
    && bash -c "$(curl -fsSL https://pacstall.dev/q/install)" \
    && pacstall --nosandbox -I go-task-deb \
    && install -m 0755 -d /etc/apt/keyrings \
    && curl -fsSL https://download.docker.com/linux/ubuntu/gpg -o /etc/apt/keyrings/docker.asc \
    && chmod a+r /etc/apt/keyrings/docker.asc \
    && echo \
        "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/debian \
        $(. /etc/os-release && echo "${UBUNTU_CODENAME:-$VERSION_CODENAME}") stable" | \
        tee /etc/apt/sources.list.d/docker.list > /dev/null \
    && apt-get update \
    && apt-get -y install docker-ce-cli docker-compose-plugin protobuf-compiler cmake age nodejs \
        libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev xdg-utils \
    && apt-get clean -y && rm -rf /var/lib/apt/lists/*
