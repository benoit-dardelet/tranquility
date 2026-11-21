ELF    := kernel/target/aarch64-unknown-none/release/kernel
IMAGE  := build/Image
DISK   := build/data.img
UNAME  := $(shell uname -s)
ifeq ($(UNAME),Darwin)
AUDIO  ?= coreaudio
else
AUDIO  ?= pa
endif
QEMU   := qemu-system-aarch64 -M virt,gic-version=3 -cpu cortex-a53 -m 256M \
          -kernel $(IMAGE) -global virtio-mmio.force-legacy=false \
          -drive if=none,file=$(DISK),format=raw,id=hd -device virtio-blk-device,drive=hd \
          -audiodev $(AUDIO),id=snd0 -device virtio-sound-device,audiodev=snd0

.PHONY: all build run run-gui test web serve clean

all: build

build:
	cd kernel && cargo build --release
	@mkdir -p build
	python3 tools/elf2image.py $(ELF) $(IMAGE)

$(DISK):
	@mkdir -p build
	truncate -s 16M $(DISK)

# Terminal phone on the serial console. Quit: Ctrl-A then X.
# No sound card on this machine? make run AUDIO=none
run: build $(DISK)
	$(QEMU) -nographic

# Graphical phone in a window (keyboard = keypad), serial log on the terminal.
run-gui: build $(DISK)
	$(QEMU) -device ramfb -device virtio-keyboard-device -serial mon:stdio

test: web
	cd phone && cargo test
	node --test web/test/*.test.mjs
	node web/e2e/installer.e2e.cjs

# Static installer site in web/dist (deploy anywhere with HTTPS).
web: build
	rm -rf web/dist && mkdir -p web/dist
	cp -r web/index.html web/style.css web/app.js web/lib web/dist/
	cp $(IMAGE).gz web/dist/tranquility-Image.gz

# WebUSB needs a secure context: http://localhost counts.
serve: web
	cd web/dist && python3 -m http.server 8000

clean:
	cd kernel && cargo clean
	cd phone && cargo clean
	rm -rf build web/dist
