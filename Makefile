SHELL := /bin/bash
.DEFAULT_GOAL := help

RUST_HOST := $(shell rustc -vV | sed -n 's/^host: //p')
export FFMPEG_DIR ?= $(CURDIR)/third_party/ffmpeg/$(RUST_HOST)

.PHONY: help ffmpeg run build test check fmt fmt-check clippy samples synthetic-samples gopro-samples

help:
	@echo 'make run                 Build and launch the app (release)'
	@echo 'make run ARGS="video.mp4" Open a video on launch'
	@echo 'make build               Build release binaries in target/release/'
	@echo 'make test                Run workspace tests'
	@echo 'make check               Check formatting and run Clippy'
	@echo 'make fmt                 Format Rust code'
	@echo 'make samples             Generate synthetic clips and fetch public GoPro clips'
	@echo 'make ffmpeg              Build the bundled FFmpeg if needed'

ffmpeg:
	bash scripts/build-ffmpeg.sh

run: ffmpeg
	cargo run --release --bin actionlay -- $(ARGS)

build: ffmpeg
	cargo build --release --workspace --bins

test: ffmpeg
	cargo test --workspace -- --test-threads=1

check: fmt-check clippy

fmt-check:
	cargo fmt --all --check

clippy: ffmpeg
	cargo clippy --workspace --all-targets -- -D warnings

fmt:
	cargo fmt --all

samples: synthetic-samples gopro-samples

synthetic-samples:
	bash scripts/make-synthetic-samples.sh

gopro-samples:
	bash scripts/fetch-gopro-samples.sh
