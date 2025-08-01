FROM node:22 AS runtime

RUN curl -sSLf -o /tmp/pnpm https://github.com/pnpm/pnpm/releases/download/v10.6.3/pnpm-linux-x64 \
    && chmod +x /tmp/pnpm \
    && ENV="/home/vscode/.bashrc" SHELL="/usr/bin/bash" /tmp/pnpm setup \
    && rm /tmp/pnpm

RUN apt-get update \
    && apt-get -y install distro-info \
    && curl -sSLf -o /tmp/task_linux_amd64.tar.gz https://github.com/go-task/task/releases/download/v3.44.1/task_linux_amd64.tar.gz \
    && tar -xf /tmp/task_linux_amd64.tar.gz -C /usr/local/bin task

RUN . /root/.bashrc \
    && pnpm install -g @anthropic-ai/claude-code \
    && pnpm cache delete

CMD ["sleep", "infinity"]
