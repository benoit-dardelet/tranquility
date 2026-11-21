KERNEL := kernel/target/aarch64-unknown-none/release/kernel
QEMU   := qemu-system-aarch64 -M virt -cpu cortex-a53 -m 128M -nographic

.PHONY: all build run test clean

all: build

build:
	cd kernel && cargo build --release

run: build
	@echo "Exit QEMU with Ctrl-A then X"
	$(QEMU) -kernel $(KERNEL)

test:
	cd phone && cargo test

clean:
	cd kernel && cargo clean
	cd phone && cargo clean
