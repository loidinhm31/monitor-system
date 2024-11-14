# Default target platform (amd64 or arm64)
PLATFORM ?= amd64

# Docker image names
IMAGE_NAME = monitor-system
AMD64_TAG = ubuntu-22.04
ARM64_TAG = bookworm-arm64

# Dockerfile paths
AMD64_DOCKERFILE = Dockerfile.amd64
ARM64_DOCKERFILE = Dockerfile.arm64

# Architecture-specific cargo target
ifeq ($(PLATFORM),arm64)
    CARGO_TARGET = aarch64-unknown-linux-gnu
    DOCKERFILE = $(ARM64_DOCKERFILE)
    TAG = $(ARM64_TAG)
else
    CARGO_TARGET = x86_64-unknown-linux-gnu
    DOCKERFILE = $(AMD64_DOCKERFILE)
    TAG = $(AMD64_TAG)
endif

.PHONY: all clean build up down up_build build_monitor_system check_platform

# Default target
all: up_build

# Check if platform is valid
check_platform:
	@if [ "$(PLATFORM)" != "amd64" ] && [ "$(PLATFORM)" != "arm64" ]; then \
		echo "Error: Invalid platform. Use PLATFORM=amd64 or PLATFORM=arm64"; \
		exit 1; \
	fi

## up: starts all containers in the background without forcing build
up: check_platform
	@echo "Starting Docker images for $(PLATFORM)..."
	DOCKERFILE=$(DOCKERFILE) docker compose up -d
	@echo "Docker images started!"

## up_build: stops docker compose (if running), builds all projects and starts docker compose
up_build: check_platform build_monitor_system
	@echo "Stopping docker images (if running...)"
	docker compose down
	@echo "Building (when required) and starting docker images..."
	DOCKERFILE=$(DOCKERFILE) docker compose up --build -d
	@echo "Docker images built and started!"

## down: stop docker compose
down:
	@echo "Stopping docker compose..."
	docker compose down
	@echo "Done!"

clean:
	@echo "Cleaning build artifacts..."
	cd ../monitor-system-service && cargo clean
	@echo "Done!"

## build_monitor_system: build the monitor system binary for the target platform
build_monitor_system: check_platform
	@echo "Building monitor-system binary for $(PLATFORM)..."
	cd ../monitor-system-service && \
    rustup target add $(CARGO_TARGET) && \
    cargo build --release --target $(CARGO_TARGET)
	@echo "Done!"

## build_image: build the Docker image for the target platform
build_image: check_platform
	@echo "Building Docker image for $(PLATFORM)..."
	docker build -t $(IMAGE_NAME):$(TAG) -f $(DOCKERFILE) .
	@echo "Done!"