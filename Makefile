IMAGE ?= ghcr.io/hexlet-codebattle/runner-rs
TAG ?= latest
PLATFORMS ?= linux/amd64,linux/arm64

.PHONY: build push lint lint-fix

## Build multi-arch image directly for GHCR
build:
	podman build \
		--platform=$(PLATFORMS) \
		--file Containerfile \
		--manifest $(IMAGE):$(TAG) \
		.

## Push multi-arch image manifest + all platform layers to GHCR
push:
	podman manifest push --all \
		$(IMAGE):$(TAG) \
		docker://$(IMAGE):$(TAG)

## Run Rust linter
lint:
	cargo clippy -- -D warnings

## Fix Rust code styling
lint-fix:
	cargo fmt
