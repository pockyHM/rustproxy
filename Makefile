SHELL := /bin/sh

CARGO ?= cargo
NPM ?= npm
DOCKER ?= docker

APP_NAME ?= rustproxy
IMAGE ?= rustproxy:local
CONFIG ?= config.yaml
DB_DIR ?= $(CURDIR)/data

.PHONY: help ui-deps ui-build build release check test fmt clean docker-build docker-run

help:
	@printf '%s\n' \
		'Targets:' \
		'  make ui-deps       Install admin UI dependencies with npm ci' \
		'  make ui-build      Build admin UI into ui/dist' \
		'  make build         Build debug binary after UI build' \
		'  make release       Build release binary after UI build' \
		'  make check         Run Rust check and UI production build' \
		'  make test          Run Rust tests and UI production build' \
		'  make fmt           Check Rust formatting' \
		'  make clean         Remove Rust and UI build outputs' \
		'  make docker-build  Build Linux container image' \
		'  make docker-run    Run container with CONFIG and DB_DIR mounted'

ui-deps:
	cd ui && $(NPM) ci

ui-build:
	cd ui && $(NPM) run build

build: ui-build
	$(CARGO) build

release: ui-build
	$(CARGO) build --release --locked

check: ui-build
	$(CARGO) check --locked

test: ui-build
	$(CARGO) test --locked

fmt:
	$(CARGO) fmt --check

clean:
	$(CARGO) clean
	rm -rf ui/dist

docker-build:
	$(DOCKER) build -t $(IMAGE) .

docker-run:
	mkdir -p $(DB_DIR)
	$(DOCKER) run --rm -it \
		-p 3000:3000 \
		-p 8080:80 \
		-v $(DB_DIR):/var/lib/rustproxy \
		-v $(CURDIR)/$(CONFIG):/etc/rustproxy/config.yaml:ro \
		$(IMAGE)
