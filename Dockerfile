# syntax=docker/dockerfile:1.7

FROM ubuntu:24.04

ARG DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    fonts-noto-cjk \
    libayatana-appindicator3-1 \
    libgomp1 \
    librsvg2-2 \
    libssl3 \
    libwebkit2gtk-4.1-0 \
    libxdo3 \
    && curl -fL "https://github.com/mayocream/koharu/releases/latest/download/koharu_linux_x64" -o /usr/local/bin/koharu \
    && chmod 0755 /usr/local/bin/koharu \
    && apt-get purge -y --auto-remove curl \
    && rm -rf /var/lib/apt/lists/*

RUN useradd --create-home --shell /bin/bash koharu \
    && install -d -o koharu -g koharu -m 755 /home/koharu/.local/share/Koharu

USER koharu
WORKDIR /home/koharu

VOLUME ["/home/koharu/.local/share/Koharu"]
EXPOSE 4000

CMD ["/usr/local/bin/koharu", "--headless", "--host", "0.0.0.0", "--port", "4000"]
