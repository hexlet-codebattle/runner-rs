# Runner-rs

Extremely fast and lightweight HTTP server for code execution written in Rust, used for [Codebattle](https://codebattle.hexlet.io).

## Features

- High-performance code execution server
- Lightweight and efficient
- Multi-architecture support (amd64 + arm64)

## Building and Deploying

### Build multi-architecture images

Build both amd64 and arm64 images into a single manifest:

```bash
make build
```

### Push to GitHub Container Registry

Push the built images to GHCR:

```bash
make push
```
