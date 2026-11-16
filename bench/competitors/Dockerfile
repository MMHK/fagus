FROM --platform=linux/amd64 python:3.13-slim
RUN apt-get update && apt-get install -y --no-install-recommends fontconfig poppler-utils && rm -rf /var/lib/apt/lists/*
COPY fonts/*.ttf /usr/share/fonts/truetype/inter/
RUN fc-cache -f && pip install --no-cache-dir --only-binary=:all: pyfulgur ironpress fullbleed plutoprint
WORKDIR /bench
